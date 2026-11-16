//! Translate normalized keys into reader commands. Shells map native key/modifier
//! values (including Command vs Control) before calling this policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    FramePrevious,
    FrameNext,
    Playback,
    Open,
    Network,
    Quit,
    Fullscreen,
    Escape,
    ToggleInterface,
    Tab,
    Next,
    Previous,
    Left,
    Right,
    First,
    Last,
    ZoomIn,
    ZoomOut,
    Fit,
    Rotate,
    Direction,
    Reload,
    Other,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Modifiers {
    pub primary: bool,
    pub shift: bool,
    pub alternate: bool,
    pub system: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    FramePrevious,
    FrameNext,
    Playback,
    Native,
    Block,
    Open { folder: bool },
    Network,
    Quit,
    Fullscreen,
    ToggleInterface,
    Page(super::PageKey),
    First,
    Last,
    ZoomIn,
    ZoomOut,
    Fit,
    Rotate { backwards: bool },
    Direction,
    Reload,
}

pub fn command(key: Key, modifiers: Modifiers, controls_have_input: bool) -> Command {
    use Command as C;
    use Key as K;
    if modifiers.primary {
        match key {
            K::Open => {
                return C::Open {
                    folder: modifiers.shift,
                };
            }
            K::Network => return C::Network,
            K::Quit => return C::Quit,
            _ => {}
        }
    }
    match key {
        K::Fullscreen => return C::Fullscreen,
        K::Escape => return C::Quit,
        K::ToggleInterface => return C::ToggleInterface,
        _ => {}
    }
    if !modifiers.primary && !modifiers.alternate && !modifiers.system {
        match key {
            K::FramePrevious => return C::FramePrevious,
            K::FrameNext => return C::FrameNext,
            K::Playback => return C::Playback,
            _ => {}
        }
    }
    if controls_have_input {
        return C::Native;
    }
    if key == K::Tab {
        return C::Block;
    }
    if modifiers.primary || modifiers.alternate || modifiers.system {
        return C::Native;
    }
    match key {
        K::Next => C::Page(super::PageKey::Next),
        K::Previous => C::Page(super::PageKey::Previous),
        K::Left => C::Page(super::PageKey::Left),
        K::Right => C::Page(super::PageKey::Right),
        K::First => C::First,
        K::Last => C::Last,
        K::ZoomIn => C::ZoomIn,
        K::ZoomOut => C::ZoomOut,
        K::Fit => C::Fit,
        K::Rotate => C::Rotate {
            backwards: modifiers.shift,
        },
        K::Direction => C::Direction,
        K::Reload => C::Reload,
        _ => C::Native,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_commands_work_with_toolbar_focus_but_not_os_modifiers() {
        for (key, expected) in [
            (Key::FramePrevious, Command::FramePrevious),
            (Key::FrameNext, Command::FrameNext),
            (Key::Playback, Command::Playback),
        ] {
            assert_eq!(command(key, Modifiers::default(), true), expected);
            assert_eq!(
                command(
                    key,
                    Modifiers {
                        primary: true,
                        ..Modifiers::default()
                    },
                    true
                ),
                Command::Native
            );
        }
    }

    #[test]
    fn editing_preserves_native_keys_but_not_window_shortcuts() {
        assert_eq!(
            command(Key::Next, Modifiers::default(), true),
            Command::Native
        );
        assert_eq!(
            command(Key::Escape, Modifiers::default(), true),
            Command::Quit
        );
        assert_eq!(
            command(Key::Tab, Modifiers::default(), false),
            Command::Block
        );
        assert_eq!(
            command(Key::Tab, Modifiers::default(), true),
            Command::Native
        );
        assert_eq!(
            command(
                Key::Open,
                Modifiers {
                    primary: true,
                    shift: true,
                    ..Modifiers::default()
                },
                true
            ),
            Command::Open { folder: true }
        );
    }

    #[test]
    fn modified_reading_keys_do_not_steal_os_shortcuts() {
        assert_eq!(
            command(
                Key::Next,
                Modifiers {
                    alternate: true,
                    ..Modifiers::default()
                },
                false
            ),
            Command::Native
        );
        assert_eq!(
            command(
                Key::Rotate,
                Modifiers {
                    shift: true,
                    ..Modifiers::default()
                },
                false
            ),
            Command::Rotate { backwards: true }
        );
    }
}
