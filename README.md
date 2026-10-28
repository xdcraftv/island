# Island for Windows

Desktop application built with Tauri, Rust, and JavaScript.

## Requirements

- Node.js and npm
- Rust
- Visual Studio Build Tools with "Desktop development with C++"
- Microsoft Edge WebView2 Runtime

## Development

```sh
npm ci
npm run dev
```

The first build compiles the Rust dependencies and can take a few minutes.
The interface lives in `src/index.html`.
Use the Island icon in the Windows system tray to open the panel or quit the application.

## Build an installer

```sh
npm run build
```

The NSIS installer is generated in `src-tauri/target/release/bundle/nsis/`.
After building, open `src-tauri/target/release/island.exe` directly, or install
Island using the NSIS installer. Running the built application does not need a
terminal, Node.js, or Rust.

If building reports `link.exe not found`, install Visual Studio Build Tools with
the **Desktop development with C++** workload, including the MSVC x64/x86 tools
and a Windows SDK. Reopen the terminal after installation and run the build again.

## Browser preview

From the project folder:

```sh
python -m http.server 8000 --bind 127.0.0.1 --directory src
```

Open `http://127.0.0.1:8000`. This previews the interface; Windows media,
hardware, clipboard, and tray features run in the desktop application.

## Repository contents

Keep source code, configuration, icons, `package-lock.json`, and
`src-tauri/Cargo.lock` in Git. Dependencies, compiled output, and generated
Tauri schemas are excluded by `.gitignore` and recreated when installing
dependencies or building the application.

Share built installers separately from the source repository.
