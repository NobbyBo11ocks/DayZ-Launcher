// Read-only statistics over the launcher's SQLite cache (node:sqlite, Node ≥ 22.5).
// Usage: node tools/cache_stats.js [path-to-cache.db]
// Default path: %LOCALAPPDATA%\com.dayzlauncher.desktop\cache.db
import path from "node:path";
import { DatabaseSync } from "node:sqlite";

const file = process.argv[2] ?? path.join(process.env.LOCALAPPDATA ?? "", "com.dayzlauncher.desktop", "cache.db");
const db = new DatabaseSync(file, { readOnly: true });
const q = (sql) => db.prepare(sql).all();
const show = (label, sql) => console.log(label.padEnd(34), JSON.stringify(q(sql)));

console.log("db:", file);
show("rows / populated(INFO>0)", "SELECT COUNT(*) AS rows, SUM(players > 0) AS populated FROM servers");
show("last refresh", "SELECT value FROM meta WHERE key = 'last_refresh'");
show("empties by no3rd", "SELECT (keywords LIKE '%no3rd%') AS fpp, COUNT(*) AS n FROM servers WHERE players = 0 GROUP BY fpp");
show("empties by privHive", "SELECT (keywords LIKE '%privHive%') AS priv, COUNT(*) AS n FROM servers WHERE players = 0 GROUP BY priv");
show("empties by external", "SELECT (keywords LIKE '%external%') AS ext, COUNT(*) AS n FROM servers WHERE players = 0 GROUP BY ext");
show("empties no3rd x privHive", "SELECT (keywords LIKE '%no3rd%') AS fpp, (keywords LIKE '%privHive%') AS priv, COUNT(*) AS n FROM servers WHERE players = 0 GROUP BY fpp, priv");
show("empties by secure", "SELECT secure, COUNT(*) AS n FROM servers WHERE players = 0 GROUP BY secure");
show("empties by password", "SELECT password, COUNT(*) AS n FROM servers WHERE players = 0 GROUP BY password");
show("empties top maps", "SELECT map, COUNT(*) AS n FROM servers WHERE players = 0 GROUP BY map ORDER BY n DESC LIMIT 8");
show("populated buckets", "SELECT CASE WHEN players <= 10 THEN '1-10' WHEN players <= 30 THEN '11-30' WHEN players <= 60 THEN '31-60' ELSE '61+' END AS b, COUNT(*) AS n FROM servers WHERE players > 0 GROUP BY b");
show("servers per IP (top 8)", "SELECT ip, COUNT(*) AS n, SUM(players > 0) AS populated FROM servers GROUP BY ip ORDER BY n DESC LIMIT 8");
show("ping buckets (ms)", "SELECT CASE WHEN ping_ms < 60 THEN '<60' WHEN ping_ms < 120 THEN '60-119' WHEN ping_ms < 250 THEN '120-249' ELSE '250+' END AS b, COUNT(*) AS n FROM servers GROUP BY b");
