//! Service : création des processus enfants (moteur, convertisseur, interface classique).
//!
//! Sous Windows, l'éditeur de release n'a pas de console (`windows_subsystem`) :
//! chaque programme console lancé en ouvrirait une nouvelle, visible. Tout
//! processus enfant passe donc par `command`, qui l'en empêche.

use std::ffi::OsStr;
use std::process::Command;

/// `Command::new`, sans fenêtre de console sous Windows.
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Rattache la sortie à la console du terminal qui a lancé l'éditeur, s'il y en a
/// une : sans console propre, `--help` et `--version` resteraient muets sous Windows.
pub fn attach_parent_console() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn AttachConsole(process_id: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        // SAFETY : appel Win32 sans pointeur ; un échec (pas de console parente) est sans effet.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}
