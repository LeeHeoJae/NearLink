# NearLink for Windows

<!-- impeccable:product-schema 1 -->

## Platform

Native Windows desktop. Android remains a separate Kotlin application.

## Stack

User-approved: Rust, Win32 native controls and tray, Windows BLE/GATT through
Microsoft windows-rs. Cargo builds a native executable.

## Users and purpose

People working at a Windows PC who want to check their phones and tablets'
battery levels without picking them up. Devices communicate locally over BLE.

## Constraints

The user prioritizes a very lightweight resident program. Use a single user-session
process, no browser engine, and event-driven battery updates. Stop discovery after
onboarding and back off retries. Preserve Android-to-Android sharing and Samsung
battery widget integration. Samsung's device-name rendering is outside this app.

## Scope

Initial Windows release receives battery information, with device aliases, optional
low/full battery notifications, reconnect, and optional login startup. Laptop battery
transmission is deferred as discussed. Real BLE interoperability requires hardware
verification; test and demo values must never be presented as live device readings.
