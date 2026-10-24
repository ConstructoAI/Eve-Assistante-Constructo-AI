//! Le groupe des programmes d'Eve : Claude Code, Piper et le serveur de l'oreille sont rattachés à un « job object »
//! de Windows qui se ferme avec Eve — fermé, tué ou planté — et les emporte. Aucun programme orphelin ne reste
//! derrière, à tenir un fichier ou un micro.
//!
//! Leurs propres enfants, eux, n'y entrent pas (JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK) : Outlook ouvert par
//! `check_setup.py --lancer`, la veille des courriels relancée par `matin.py`, un script en plein `--ecrire` survivent à
//! la fermeture d'Eve (la revue de robustesse du 2026-10-21 : fermer Eve tuait Outlook et la veille). Les serveurs MCP
//! de Claude Code se ferment d'eux-mêmes quand leur entrée se ferme.

use std::process::Child;

#[cfg(windows)]
mod imp {
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::sync::OnceLock;

    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    use windows::core::PCWSTR;

    struct Groupe(HANDLE);
    // SAFETY : un handle de job object se partage entre fils ; il n'est jamais fermé avant la fin du programme.
    unsafe impl Send for Groupe {}
    unsafe impl Sync for Groupe {}

    static GROUPE: OnceLock<Option<Groupe>> = OnceLock::new();

    fn groupe() -> Option<&'static Groupe> {
        GROUPE
            .get_or_init(|| {
                // SAFETY : un job object anonyme, puis sa limite « tuer à la fermeture » ; la structure vit le temps
                // de l'appel.
                unsafe {
                    let h = CreateJobObjectW(None, PCWSTR::null()).ok()?;
                    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                    info.BasicLimitInformation.LimitFlags =
                        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
                    SetInformationJobObject(
                        h,
                        JobObjectExtendedLimitInformation,
                        (&raw const info).cast(),
                        std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                    )
                    .ok()?;
                    Some(Groupe(h))
                }
            })
            .as_ref()
    }

    pub fn rattacher(enfant: &Child) {
        if let Some(g) = groupe() {
            // SAFETY : le handle du processus enfant est valide tant que `enfant` vit.
            let _ = unsafe { AssignProcessToJobObject(g.0, HANDLE(enfant.as_raw_handle())) };
        }
    }
}

#[cfg(windows)]
pub fn rattacher(enfant: &Child) {
    imp::rattacher(enfant);
}

#[cfg(not(windows))]
pub fn rattacher(_enfant: &Child) {}
