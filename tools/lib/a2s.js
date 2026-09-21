// Shared A2S helpers for the tools (ESM; the root package.json is `type: module`).
// Protocol references: docs/03-server-discovery-and-a2s.md (S-15 Valve wiki, S-12/S-14 parsers, S-42 live).
import dgram from "node:dgram";

const HDR_SINGLE = -1; // 0xFFFFFFFF
const HDR_SPLIT = -2; // 0xFFFFFFFE

export const buildInfo = (ch) =>
  Buffer.concat([Buffer.from([0xff, 0xff, 0xff, 0xff, 0x54]), Buffer.from("Source Engine Query\0", "latin1"), ch ?? Buffer.alloc(0)]);
export const buildRules = (ch) => Buffer.concat([Buffer.from([0xff, 0xff, 0xff, 0xff, 0x56]), ch ?? Buffer.from([0xff, 0xff, 0xff, 0xff])]);
export const buildPlayer = (ch) => Buffer.concat([Buffer.from([0xff, 0xff, 0xff, 0xff, 0x55]), ch ?? Buffer.from([0xff, 0xff, 0xff, 0xff])]);

/**
 * Sends one query, transparently answering a challenge, and collects every datagram
 * of the final response. Split responses resolve when all fragments (12-byte header
 * assumption) arrived, or 400 ms after the last datagram so unknown layouts are
 * still captured for inspection.
 */
export function query(host, port, build, timeoutMs = 4000) {
  return new Promise((resolve, reject) => {
    const sock = dgram.createSocket("udp4");
    let challenge = null;
    let t0 = 0n;
    let rtt = 0;
    let timer;
    let settle;
    let datagrams = [];
    const finish = () => {
      clearTimeout(timer);
      clearTimeout(settle);
      sock.close();
      resolve({ datagrams, rtt, ...reassemble(datagrams) });
    };
    const send = () => {
      datagrams = [];
      t0 = process.hrtime.bigint();
      sock.send(build(challenge), port, host);
    };
    sock.on("message", (msg) => {
      if (datagrams.length === 0) rtt = Number(process.hrtime.bigint() - t0) / 1e6;
      const hdr = msg.readInt32LE(0);
      if (hdr === HDR_SINGLE && msg[4] === 0x41 && datagrams.length === 0) {
        challenge = Buffer.from(msg.subarray(5, 9));
        send();
        return;
      }
      datagrams.push(Buffer.from(msg));
      if (hdr === HDR_SINGLE) return finish();
      if (hdr === HDR_SPLIT) {
        const total = msg[8];
        if (datagrams.length >= total) return finish();
        clearTimeout(settle);
        settle = setTimeout(finish, 400);
      }
    });
    sock.on("error", (e) => {
      clearTimeout(timer);
      clearTimeout(settle);
      reject(e);
    });
    timer = setTimeout(() => {
      clearTimeout(settle);
      sock.close();
      reject(new Error(`timeout after ${timeoutMs} ms`));
    }, timeoutMs);
    send();
  });
}

/** Reassembles datagrams into `{ data, packets, split }` where data excludes the FF FF FF FF header. */
export function reassemble(datagrams) {
  if (datagrams.length === 0) return { data: null, packets: 0, split: false };
  const first = datagrams[0];
  if (first.readInt32LE(0) === HDR_SINGLE) return { data: first.subarray(4), packets: 1, split: false };
  const frags = datagrams
    .map((d) => ({ id: d.readUInt32LE(4), total: d[8], num: d[9], size: d.readUInt16LE(10), body: d.subarray(12) }))
    .sort((a, b) => a.num - b.num);
  const full = Buffer.concat(frags.map((f) => f.body));
  const compressed = (frags[0].id & 0x80000000) !== 0;
  const data = full.readInt32LE(0) === HDR_SINGLE ? full.subarray(4) : full;
  return { data, packets: frags.length, split: true, total: frags[0].total, splitSize: frags[0].size, compressed };
}

const cstr = (b, o) => {
  const e = b.indexOf(0, o);
  if (e < 0) throw new Error("unterminated string");
  return [b.toString("utf8", o, e), e + 1];
};

export function parseInfo(b) {
  let o = 0;
  const r = {};
  if (b[o++] !== 0x49) throw new Error("not INFO 0x" + b[0].toString(16));
  r.protocol = b[o++];
  [r.name, o] = cstr(b, o);
  [r.map, o] = cstr(b, o);
  [r.folder, o] = cstr(b, o);
  [r.game, o] = cstr(b, o);
  r.appId16 = b.readUInt16LE(o);
  o += 2;
  r.players = b[o++];
  r.maxPlayers = b[o++];
  r.bots = b[o++];
  r.serverType = String.fromCharCode(b[o++]);
  r.environment = String.fromCharCode(b[o++]);
  r.password = !!b[o++];
  r.vac = !!b[o++];
  [r.version, o] = cstr(b, o);
  const edf = b[o++];
  r.edf = edf;
  if (edf & 0x80) {
    r.gamePort = b.readUInt16LE(o);
    o += 2;
  }
  if (edf & 0x10) {
    r.steamId = b.readBigUInt64LE(o).toString();
    o += 8;
  }
  if (edf & 0x40) {
    r.specPort = b.readUInt16LE(o);
    o += 2;
    [r.specName, o] = cstr(b, o);
  }
  if (edf & 0x20) [r.keywords, o] = cstr(b, o);
  if (edf & 0x01) {
    r.gameId = b.readBigUInt64LE(o).toString();
    o += 8;
  }
  r.trailingBytes = b.length - o;
  return r;
}

export function unescapeDayz(buf) {
  const out = [];
  for (let i = 0; i < buf.length; i++) {
    if (buf[i] === 0x01 && i + 1 < buf.length) {
      const n = buf[i + 1];
      if (n === 0x01) {
        out.push(0x01);
        i++;
        continue;
      }
      if (n === 0x02) {
        out.push(0x00);
        i++;
        continue;
      }
      if (n === 0x03) {
        out.push(0xff);
        i++;
        continue;
      }
    }
    out.push(buf[i]);
  }
  return Buffer.from(out);
}

export function parseRules(b) {
  let o = 0;
  if (b[o++] !== 0x45) throw new Error("not RULES 0x" + b[0].toString(16));
  const count = b.readUInt16LE(o);
  o += 2;
  const plain = {};
  const chunks = [];
  for (let i = 0; i < count; i++) {
    const ke = b.indexOf(0, o);
    const key = b.subarray(o, ke);
    o = ke + 1;
    const ve = b.indexOf(0, o);
    const val = b.subarray(o, ve);
    o = ve + 1;
    if (key.length === 2 && key[0] <= key[1]) chunks.push({ idx: key[0], total: key[1], val });
    else plain[key.toString("latin1")] = val.toString("utf8");
  }
  chunks.sort((a, c) => a.idx - c.idx);
  const raw = Buffer.concat(chunks.map((c) => c.val));
  const p = unescapeDayz(raw);
  const d = { ruleCount: count, chunkCount: chunks.length, chunkKeys: chunks.map((c) => `${c.idx}/${c.total}`), rawLen: raw.length, unescapedLen: p.length };
  if (p.length === 0) return { ...d, plainRules: plain };
  let q = 0;
  d.protocolVersion = p[q++];
  d.overflow = p[q++];
  d.dlcFlags = p.readUInt16LE(q);
  q += 2;
  const pop = d.dlcFlags.toString(2).split("1").length - 1;
  d.dlcHashes = [];
  for (let i = 0; i < pop; i++) {
    d.dlcHashes.push(p.readUInt32LE(q));
    q += 4;
  }
  const mc = p[q++];
  d.mods = [];
  for (let i = 0; i < mc; i++) {
    const hash = p.readUInt32LE(q);
    q += 4;
    const lenByte = p[q++];
    const len = lenByte & 0x0f;
    let id = 0n;
    for (let k = 0; k < len; k++) id |= BigInt(p[q + k]) << BigInt(8 * k);
    q += len;
    const nl = p[q++];
    const name = p.toString("utf8", q, q + nl);
    q += nl;
    d.mods.push({ hash, idLenByte: lenByte, steamWorkshopId: id.toString(), name });
  }
  const sc = p[q++];
  d.signatures = [];
  for (let i = 0; i < sc; i++) {
    const l = p[q++];
    d.signatures.push(p.toString("utf8", q, q + l));
    q += l;
  }
  if (q < p.length) {
    const l = p[q++];
    d.description = p.toString("utf8", q, q + l);
    q += l;
  }
  d.trailingBytes = p.length - q;
  d.plainRules = plain;
  return d;
}

export function parsePlayers(b) {
  let o = 0;
  if (b[o++] !== 0x44) throw new Error("not PLAYER 0x" + b[0].toString(16));
  const count = b[o++];
  const players = [];
  for (let i = 0; i < count && o < b.length; i++) {
    const index = b[o++];
    let name;
    [name, o] = cstr(b, o);
    const score = b.readInt32LE(o);
    o += 4;
    const duration = b.readFloatLE(o);
    o += 4;
    players.push({ index, name, score, duration });
  }
  return { count, players, trailingBytes: b.length - o };
}

/** DayZ keyword tags (docs/03 §3). */
export function parseKeywords(s) {
  const tags = { unknown: [] };
  for (const t of (s ?? "").split(",").filter(Boolean)) {
    if (t === "battleye") tags.battleye = true;
    else if (t === "no3rd") tags.firstPersonOnly = true;
    else if (t === "external") tags.external = true;
    else if (t === "privHive") tags.privateHive = true;
    else if (t === "mod") tags.modded = true;
    else if (t === "isDLC") tags.dlc = true;
    else if (t.startsWith("shard")) tags.shard = t.slice(5);
    else if (t.startsWith("lqs")) tags.queue = Number(t.slice(3));
    else if (t.startsWith("etm")) tags.timeMultiplier = Number(t.slice(3));
    else if (t.startsWith("entm")) tags.nightMultiplier = Number(t.slice(4));
    else if (/^\d{1,2}:\d{2}$/.test(t)) tags.time = t;
    else tags.unknown.push(t);
  }
  return tags;
}
