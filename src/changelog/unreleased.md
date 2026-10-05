The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

The sections should follow the order `Added`, `Changed`, `Deprecated`,
`Removed`, and `Fixed`.

Platform specific changed should be added to the end of the section and grouped
by platform name. Common API additions should have `, implemented` at the end
for platforms where the API was initially implemented. See the following example
on how to add them:

```md
### Added

- Add `Window::turbo()`, implemented on X11, Wayland, and Web.
- On X11, add `Window::some_rare_api`.
- On X11, add `Window::even_more_rare_api`.
- On Wayland, add `Window::common_api`.
- On Windows, add `Window::some_rare_api`.
```

When the change requires non-trivial amount of work for users to comply
with it, the migration guide should be added below the entry, like:

```md
- Deprecate `Window` creation outside of `EventLoop::run`

  This was done to simply migration in the future. Consider the
  following code:

  // Code snippet.

  To migrate it we should do X, Y, and then Z, for example:

  // Code snippet.

```

The migration guide could reference other migration examples in the current
changelog entry.

## Unreleased

### Added

- On Wayland, add `WindowExtWayland::kora_identity()` and `WindowEvent::IdentityChanged`
  for compositor-authenticated Kora window identifiers and process workspaces.
- On Wayland, add `WindowExtWayland::set_app_commands()`, `request_app_palette()` and
  `WindowEvent::AppCommand`, so a window publishes its commands and recent items to
  Kora's Halo (`kora_app_commands_v1`) and runs the ones picked there.

### Fixed

- On macOS, fix crash on macOS 26 by using objc2's `relax-sign-encoding` feature.
