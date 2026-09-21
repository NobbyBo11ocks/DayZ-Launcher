# DayZ Launcher · project documentation

Windows-only DayZ Standalone launcher: Tauri 2 (Rust) + Svelte 5, key-free server list from Steam, direct A2S queries, one-click Workshop mod sync, launch through `DayZ_BE.exe`. Research phase completed 2026-09-21.

## Index

| Doc | Contents |
|---|---|
| [01-competitor-analysis.md](01-competitor-analysis.md) | DZSA, launchZ, Beans, dayz-ctl: what to adopt and avoid, feature matrix |
| [02-dayz-launch-mechanics.md](02-dayz-launch-mechanics.md) | Steam/registry/VDF discovery, Workshop layout, `!Workshop` junctions, launch line, parameters, UGC flow |
| [03-server-discovery-and-a2s.md](03-server-discovery-and-a2s.md) | Server list sources, A2S INFO/RULES/PLAYER, DayZ Server Browser Protocol v2 (live-verified), DZSA API |
| [04-tech-stack-decision.md](04-tech-stack-decision.md) | ADR-001 stack with verified versions, ADR-002 Steam integration |
| [05-architecture-and-optimisation.md](05-architecture-and-optimisation.md) | Modules, data flow, IPC, budgets, per-file optimisation checklist |
| [06-ui-design.md](06-ui-design.md) | Layout, table, details, filters, tokens, themes, states, accessibility |
| [07-roadmap.md](07-roadmap.md) | Milestones M0–M7 with acceptance criteria |
| [08-sources.md](08-sources.md) | Every source with access date and confidence |
| [09-decisions-log.md](09-decisions-log.md) | Append-only decisions and verifications |
| [10-open-questions.md](10-open-questions.md) | Unresolved items and how to resolve them |
| [11-fake-population-detection.md](11-fake-population-detection.md) | Spoofed player counts: mechanism, live measurement, detection rules R1–R7 |
| [../tools/a2s_probe.js](../tools/a2s_probe.js) | Live A2S probe used to verify the protocol (`node tools/a2s_probe.js <ip> <queryPort>`) |

## Working rules

1. **Source before patch.** Any change to protocol, launch, Steam, dependency versions or performance-sensitive code starts with re-reading the cited source (08) and adding a row to 09.
2. **Verify live where possible.** Protocol claims are checked with `tools/a2s_probe.js`; file-layout claims against this machine's Steam install; versions against the registries with the commands in 04 §5.
3. **Optimise per file.** Apply the checklist in 05 §7 to every file touched; log deviations.
4. **Match mods by Workshop ID**, never by name (02 §3).
5. **Keep the official launcher working.** Reuse its `!Workshop` junctions; never delete or rename them.
6. **No telemetry, no accounts, no remote code.**
