# Performance and idle behavior

Google Photos Sync is a native Rust and Win32 application. It does not embed a browser engine, JavaScript runtime, web server, or always-running background service.

## Idle design

- The progress animation timer exists only while a sync or background action is active.
- The hidden window is not invalidated or repainted.
- The schedule check runs once per minute; uploads still follow each folder's configured interval.
- A silent Windows autostart does not walk every media folder merely to populate hidden dashboard counters.
- If no folder is due, startup does not create a sync worker.
- Opening the dashboard loads accurate counters on demand.
- The daily update network check runs in a short-lived copy of the same Rust executable. Native TLS threads and pages exit with that worker instead of remaining in the tray process.
- Release builds use size optimization, full LTO, one code generation unit, stripped symbols, and abort-on-panic behavior.

## Reference measurement

Measured on Windows 11 with the same existing configuration, hidden tray window, no upload, and a 30-second observation interval:

| Build | CPU time during interval | Working set | Private memory | Handles | Threads |
| --- | ---: | ---: | ---: | ---: | ---: |
| Before idle optimization | 0.03125 s | 17.41 MiB | 3.13 MiB | 261 | 2 |
| Optimized release build | 0.00000 s | 12.52 MiB | 2.25 MiB | 160 | 2 |
| Isolated updater, after the daily network check | 0.00000 s | 12.71 MiB | 2.22 MiB | 162 | 1 |

The final optimized executable was 2,235,392 bytes (2.13 MiB). Windows memory accounting and background conditions vary, so these values are a reference measurement rather than a fixed system requirement.

The performance changes do not weaken duplicate protection. Real uploads remain blocked until a Takeout import has indexed older Google Photos content or the user explicitly confirms that the selected folders have no older copies in Google Photos.
