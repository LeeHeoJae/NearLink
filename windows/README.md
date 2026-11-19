# NearLink for Windows

English · [한국어](README.ko.md)

A native Rust + Win32 app that receives the battery level and charging state of phones and tablets over BLE and shows them in the system tray. It uses no web engine, JVM or .NET runtime. Sending a laptop's battery is not part of this version.

## Run

Developed for Windows 11 x64 with a BLE adapter. Windows 10 and ARM64 have not been verified yet. Receiving a battery does not need the BLE peripheral role.

1. Install the latest Android NearLink and tap **Prepare to connect**.
2. Run `nearlink.exe` and click **Add device**.
3. Select a discovered device and click **Connect selected device**. The search ends after 15 seconds.
4. For a device that isn't paired yet, the app asks Windows to pair. Confirm the request on both sides. If a number comparison is needed, check that the NearLink confirmation window shows the same code as Android. If pairing is canceled or fails, the app shows the detailed cause and stops automatic reconnection. Click **Reconnect** to try again from pairing. When the preparation time runs out, prepare again on Android.
5. In the list, check the level, charging state, connection state and the time the battery was last received.

**You need the Android build from this repository.** Earlier versions don't support the command that registers a receive-only PC. When connecting, Windows performs JOIN_RECEIVER → HELLO/READY → read and subscribe → ACK, and shows Connected only after the ACK write succeeds and a later battery notification arrives.

## Usage

- Left-click the tray icon: a small **Device Battery** window. It shows each device's level and lets you open **Add device** or **Open details**. Right-click: the Windows default menu with device management, settings and exit.
- The default tray icon is a hollow single-color battery that follows the taskbar's light/dark theme and high contrast colors. Right-click → **Tray icon** and choose a main device to show its level (%) as a number. The choice is kept across restarts. Lost connections, stopped receiving, not yet received and values older than 15 minutes return to the default outline. Hovering shows the main device first with level, charging and connection state. Left-click opens the full list.
- The management window shows each device's name, level and charging state, connection state and last reception in one list item. Selecting a device enables the actions below the list. It supports arrow keys, Tab, Alt access keys and a per-device context menu.
- **Nickname**: sets the name shown on this PC. The name in the Samsung widget doesn't change.
- **Stop receiving**: stops receiving on this PC and stays stopped after a restart.
- **Reconnect**: turns a stopped device back on or retries the connection.
- **Remove from list**: removes the registration on this PC. The Windows pairing is kept.
- To also end Android's registration and sending service, stop that device's connection in the Android app.
- Closing the window keeps the app running in the tray. Choose Exit from the tray menu to quit.
- In Settings you can choose the low battery threshold (1–90%), the 100% alert and starting at sign-in. Starting at sign-in is off by default. When on, it registers the current executable path in the user's Run key. Turn it off before moving the file, then turn it on again at the new location.
- Alerts use Windows tray notifications and may not appear depending on Do Not Disturb and notification settings. When several devices raise alerts at once, they are combined into one notification. A device whose charging state is unknown still raises one low battery alert; it is skipped only when charging is confirmed.
- The Add device window marks devices already registered on this PC as **Already added**.

The last value of a disconnected device is kept as history. **Check the connection state together with the last received time.** The last time is based on battery data reception, so it can be old if the battery doesn't change. Restarting doesn't restore an old level as if it were current.

If battery data hasn't been updated for 15 minutes or more, **Outdated value** is shown. However, if the 30-second session check of a connected device succeeded within the last 15 minutes, Android simply sent no notification because nothing changed, so the value is shown as current. A lost connection is shown as **Disconnected · Waiting to reconnect** and stopped receiving as **Receiving stopped**. An old reception time alone is never taken as a lost connection. Times older than a day are shown as "Yesterday" and "N days ago".

The app follows Windows light/dark app mode, the accessibility text size (100–225%) and high contrast colors. Windows draws the title bar and menus. The default scroll bars and message boxes keep the OS's standard look. The window fits the monitor's work area and the device list scrolls vertically.

## Languages

The app follows the Windows display language: Korean when it is Korean, English otherwise. To force a language, start it with `--lang en` or `--lang ko`, or set `NEARLINK_LANG`. See [Adding a language](../README.md#adding-a-language) to add another one.

## Lightweight behavior

- One process in the user session and one hidden message window keep the tray alive.
- Device search runs only in the Add device window and stops when the search ends, the window closes or the app exits.
- One worker thread per device waits for GATT change notifications. It doesn't poll the battery repeatedly.
- Each connection attempt owns a keep-alive GATT session and releases it on retry, stop and exit. On exit all device tasks are canceled and the single-instance lock is released after cleanup finishes.
- While connected, the registration and server session are checked every 30 seconds. On failure it retries with 1→2→4→… up to 60 seconds.
- GATT operations run in order per device and are canceled after 20 seconds. The first-pairing limit is 60 seconds.
- When a server service change, disconnection or session change is detected, event registrations and old handles are released and it reconnects.
- Closing the window stops the redraw timer. A small amount of UI resources is kept for reopening.
- Files are saved only when settings or device names change. No battery alert writes to disk.
- The C runtime is linked statically. No runtime other than the DLLs shipped with the OS needs to be installed.

## Settings location

`%LOCALAPPDATA%\NearLink\settings.json` stores device identifiers, names and nicknames, whether receiving is enabled, and alert settings. A temporary file is written and synced, then replaced atomically. A file that can't be read is not overwritten and an error is shown. Nothing is sent to an external server.

For development checks, `nearlink.exe --data-dir <folder>` uses a separate profile. Only one instance runs per profile, and a second launch opens the existing management window.

## Build and verification

You need the Rust MSVC toolchain, the Visual Studio **Desktop development with C++** build tools and the Windows SDK. From the repository root:

```powershell
.\windows\build.ps1
.\windows\build.ps1 test
.\windows\build.ps1 check
```

The build output is `windows/target/release/nearlink.exe`. `Cargo.lock` is included and builds use `--locked`. If a repository-local `.tools` toolchain installed for this work exists, the script uses it automatically; otherwise it uses the Cargo on PATH.

The UI uses Windows standard controls, the system font, keyboard navigation and DPI scaling. It is laid out around the device list, and search, nickname and settings use separate owned windows. Real BLE reception, reboot and sleep recovery, and long runs with several devices need additional verification on that hardware.

[Initial verification results and resource measurements](../docs/windows-verification.md) (Korean) record what was checked and what remains.

[Windows 11 UI improvement verification](../docs/windows-ui-verification.md) (Korean) records screen and interaction checks and before/after resource comparisons.

Sample data for screen checks is available only in a separate profile. It isn't real reception and runs no BLE, notifications, startup changes or settings saves.

```powershell
.\windows\target\release\nearlink.exe --preview --data-dir .\ui-review\windows\preview
# --preview-theme light|dark|contrast, --preview-text-scale 150, --preview-empty
# --lang en|ko : force the display language
# --quick-view : start in the quick view (also available in normal runs)
```
