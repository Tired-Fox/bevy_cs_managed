# Bevy CS Managed

A Bevy plugin that adds a .NET C# Scripting runtime

> [!WARNING]
> This project is in early development and is very experimental

Current progress:
  - Compiling custom runtime, engine api, and scripts
  - Loading engine api and scripts
  - Binding runtime api
  - Loads class from scripts
  - Executes method on Class instance on component add and on update

> Look at `src/bin/simple.rs`, `assets/engine/Bevy.cs`, and `assets/scripts/Player.cs` for example implementation

Run with `cargo run --bin=simple` and optionally add `--features=always-compile-runtime` to force compile the custom runtime each build

![0.0.0 Progress Update](/readme/0.0.0-progres-update.gif)

### What it does

1. Finds an appropriate .Net version on the users system
2. Builds the runtime, engine, and scripts into dll files
3. Loads and manages dll files
4. Hot re-compile scripts
5. Binds script methods to lifetime and event hooks
6. Customizable to the users components and setup

### Limitations

Currently the .Net Runtime distrobution format of this plugin only supports desktop targets (windows, linux, macos). With more research this may expand to
mobile and web, or making it compatible with [Mono](https://www.mono-project.com/) could allow for Mono to be used for mobile and web targets.

### Unknowns

- How to dynamically bind and generate the `Engine` C# api. This includes user defined types that implement `Reflect` and is `Reflectable`
- Will this plugin provide most of bevy builtin types or will the user have to expose what they want?
- How third party plugins and types can be reflected and registered
- Dynamically generate the .Net project file, `csproj` and `sln`, so it imports the `Engine` api
- How to bind method params both for objects and native types as a `object?[]` to send to `MethodInfo.Invoke`
- How to map structs and enums to c# types. When they are passed to c# managed code how will modification and interaction work?

### Examples

1. Simple:
    - `cargo run --example simple` **WITH** dynamically compiled `.cs` files
    - `cargo run --example simple -F distribute` **WITHOUT** dynamically compiled `.cs` files

### Todo

- [x] Find .Net and Hostfxr version
- [ ] Write a .Net runtime similar to `Mono` to allow for the following:
    - [x] scoping
    - [x] loading dll
    - [x] searching and validating types
    - [x] fetching and calling methods
    - [ ] Error return values
    - [ ] Optimal memory management.
        - Is `GCHandle.Alloc` with `GCHandle.Free` on `Drop` good enough?
        - Is it better to find a way to make the data returned raw pointers which can be pinned with the runtime api similar to Mono?
- [x] Compiled runtime dll
    - [x] Compiled once for the given .Net version
- [x] Bind runtime
- [ ] Bind interop functions
- [ ] Bind interop data
- [ ] Generate Engine API bindings
- [ ] Compile Engine API
    - [x] Compile dll to `target` directory
    - [ ] Gracefully handle errors
- [x] Load Engine API
- [ ] Compile user scripts
    - [x] Compile dll to `target` directory
    - [ ] Gracefully handle errors
- [x] Load user Scripts
- [x] Bind user script methods to hooks
- [ ] Hot reload and compile user scripts on file changes
- [ ] Build script for distrobution (production) builds
    - [ ] Lock behind feature flag
    - [ ] Bundle the users selected .Net
    - [ ] Compiled and bundle
        - [x] Runtime dll and runtimeconfig
        - [ ] Engine.dll
        - [ ] Scripts.dll
- [ ] Optionally use [Mono]() for all platforms or mobile and web

### Notes

- Opting to bind Runtime methods by writing a delegate signature with `[UnmanagedFunctionPointer(CallingConvention.Cdecl)]` and a matching method with the same signature to allow for `out` c# params
- C# `Engine` namespace will have a static `Interop` class with a table of unmanaged delegates that point to rust functions that can be called from Managed code.
- C# `Engine` namespace will have a static `Interop` class with a pointer to the current `world` to allow for context sensitive `interop` calls to manipulate and query the `world`
