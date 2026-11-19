# NearLink

English · [한국어](README.ko.md)

NearLink is an experimental app that exchanges the battery level and charging state of several Android devices over Bluetooth Low Energy (BLE) and shows them in the default Samsung battery widget. It does not provide a widget of its own.

On Windows, a [Rust + Win32 tray app](windows/README.md) can receive the battery of Android devices.
The first Windows version is receive-only and needs the Android build from this repository. Independently of the Samsung widget's naming, the Windows list shows each device's name or nickname. Real BLE behavior needs verification on real devices.

<p align="center">
  <img src="docs/screenshots/samsung-battery-widget-cropped.svg" width="720" alt="The default Samsung battery widget showing the battery levels of two devices">
  <br>
  <sub>Example of the default Samsung battery widget</sub>
</p>

> **Experimental:** Widget display was confirmed with a build that used an earlier app ID. Multi-device connections, widget compatibility of the current version, recovery after reboot and long-term stability still need verification on real devices. What is shown can differ by device and software version.

## Languages

The Android app and the Windows tray app follow the system language. English is the default and Korean is included. Other languages fall back to English.

- Android: choose the language in the system settings (Android 13 and later offer per-app language under **Settings → System → Languages**).
- Windows: the app follows the Windows display language. To force one, start it with `--lang en` or `--lang ko`, or set the `NEARLINK_LANG` environment variable.

To add a language, see [Adding a language](#adding-a-language).

## Requirements

- Two or more Android 8.0 (API 26) or later devices that support BLE
- The latest NearLink installed on each device, with Bluetooth turned on
- A Samsung device on which to check the default battery widget

## How to use

1. Install the app on both devices and turn on Bluetooth.
2. On one device, tap **Prepare to connect**.
3. Within 2 minutes, tap **Add device** on the other device and choose the prepared device. Confirm the pairing request on both devices when it appears.
4. When the connection completes, check the other device's battery in the app on both sides. On Samsung devices, check the default battery widget as well.
5. Connect additional devices the same way. For every device to see every other device's battery, add each pair of devices directly. Batteries are not relayed through other devices.

When the preparation time runs out, tap **Prepare to connect** again. If you add the **Prepare to connect** tile to the Quick Settings panel, it opens the app and starts preparing right away. You can also choose an already paired device from **Add device → Connect paired device**; devices you have connected with NearLink, and phones and tablets, are listed first. For a device that doesn't support adding devices in the app, pair it in the system Bluetooth settings first.

Searching for devices needs the Nearby devices permission, and Android 11 and earlier may need the Location permission. On Android 13 and later the app also asks for the notification permission to show connection status and battery alerts, but connections work if you decline. If Bluetooth is off or a permission is denied, the message lets you turn it on or open the app settings right away. If searching doesn't work, also check that location services are on. While connected you can choose to exclude the app from battery optimization, which may increase battery usage. If reconnection keeps repeating, the battery screen guides you again.

In each device's **Device settings** you can set a nickname used only in this app, or remove a stopped device from the list. Removing it deletes the saved battery history and nickname; the Bluetooth pairing is kept. The device name in the Samsung widget doesn't change. In **Battery alerts** you can set low battery (5–50%) and fully charged alerts for connected devices.

When a connection drops, the app tries to reconnect each device. You can stop each device, or stop all connections after confirming, and the persistent notification's **Stop all connections** works too. The other device is asked to stop as well, but if the connection is lost or the other app doesn't support the per-device stop command, stop it on the other device yourself. Older versions can connect to only one device, so update all devices to the latest version. The last battery info saved in the app can differ from the current state.

To investigate Samsung battery widget refresh problems, see [How to collect battery diagnostic logs](docs/battery-diagnostics.md) (Korean).

## Build and test

You need Android Studio, JDK 17 or later and Android SDK Platform 35. If the SDK isn't detected automatically, set it in Android Studio or set `ANDROID_HOME`.

```powershell
# Windows
.\gradlew.bat assembleDebug
.\gradlew.bat testDebugUnitTest
```

On macOS and Linux run `./gradlew assembleDebug` and `./gradlew testDebugUnitTest` respectively. The built APK is created at `app/build/outputs/apk/debug/app-debug.apk`.

## Adding a language

- **Android:** copy `app/src/main/res/values/strings.xml` to `app/src/main/res/values-<code>/strings.xml` (for example `values-ja`), translate the values, and list the language in `app/src/main/res/xml/locales_config.xml`. Keep the format placeholders (`%1$d`, `%1$s`) and translate the `<plurals>` entries too.
- **Windows:** add a variant to `Lang` in `windows/src/i18n.rs`, parse it in `parse`/`detect`, and add a translation table next to `KO`. English text is the key; `cargo test` checks that every message used in the code has a translation and that placeholders (`{0}`, `{1}`) match.

## License

[MIT License](LICENSE)
