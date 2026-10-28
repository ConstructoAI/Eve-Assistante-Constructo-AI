//! Les notifications de Windows : ce qu'Eve a à dire quand sa fenêtre n'est pas sous les yeux — un rappel qui sonne,
//! une réponse finie pendant qu'on était ailleurs, un accord qu'elle attend —, dans le coin de l'écran comme celles des
//! autres programmes, et dans le centre de notifications ; un clic sur l'une ramène sa fenêtre (le geste `notification`,
//! que le chef reçoit comme un geste de la fenêtre).
//!
//! Windows ne montre les notifications que d'un programme qu'il connaît : Eve s'inscrit sous un identifiant à elle
//! (`AppUserModelId`, dans le registre de la personne : son nom affiché et son icône, le logo de sa fenêtre écrit dans
//! son dossier local), à chaque démarrage. Elles se montrent sans son : Eve sonne déjà elle-même. Le réglage
//! `notifications` (vrai d'office) les coupe ; le chef décide quand en montrer une.

use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::thread;

use crate::chef::Evt;

/// Le fichier de l'icône des notifications, dans le dossier local.
const ICONE: &str = "notifications.png";
/// Les bornes d'une notification : son titre, son texte (Windows en coupe d'ailleurs le reste).
const TITRE_MAX: usize = 60;
const TEXTE_MAX: usize = 200;

/// Le fil des notifications : on lui donne un titre et un texte, il les montre.
pub struct Notifications {
    envoi: Option<Sender<(String, String)>>,
}

impl Notifications {
    /// Le fil des notifications d'Eve : son identifiant d'après son dossier local (`local`), son nom affiché (`nom`) ;
    /// un clic sur une notification rend le geste `notification` au chef (`rapport`). Sans Windows, rien.
    pub fn demarrer(nom: &str, local: &Path, rapport: Sender<Evt>) -> Notifications {
        let (envoi, recus) = mpsc::channel::<(String, String)>();
        let (nom, local) = (nom.to_string(), local.to_path_buf());
        let lance = thread::Builder::new().name("notifications".into()).spawn(move || {
            imp::mener(&identifiant(&local), &nom, &local.join(ICONE), &recus, &rapport);
        });
        Notifications { envoi: lance.is_ok().then_some(envoi) }
    }

    /// Montre une notification : son titre, son texte (coupés s'ils sont trop longs).
    pub fn montrer(&self, titre: &str, texte: &str) {
        if let Some(e) = &self.envoi {
            let _ = e.send((ecourter(titre, TITRE_MAX), ecourter(texte, TEXTE_MAX)));
        }
    }
}

/// L'identifiant d'une Eve auprès de Windows : celui de Constructo AI, puis le nom de son dossier local (une Eve par
/// spécialiste, chacune ses notifications), en lettres, chiffres, points et tirets.
fn identifiant(local: &Path) -> String {
    let nom = local.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let nom: String = nom.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).collect();
    let nom = nom.trim_matches('-');
    if nom.is_empty() { "ConstructoAI.Eve".into() } else { format!("ConstructoAI.Eve.{nom}") }
}

/// Un texte coupé à `max` caractères au plus, entre deux mots, et « … » s'il l'a été ; ses sauts de ligne en espaces.
fn ecourter(texte: &str, max: usize) -> String {
    let texte = texte.split_whitespace().collect::<Vec<_>>().join(" ");
    if texte.chars().count() <= max {
        return texte;
    }
    let coupe: String = texte.chars().take(max - 1).collect();
    let coupe = coupe.rsplit_once(' ').map_or(coupe.as_str(), |(debut, _)| debut).trim_end_matches([',', ';', ':']);
    format!("{coupe}…")
}

/// Le XML d'une notification : son titre, son texte, sans son, et ce qu'un clic rend (`launch`).
fn xml_de(titre: &str, texte: &str) -> String {
    format!(
        "<toast launch=\"eve\"><visual><binding template=\"ToastGeneric\"><text>{}</text><text>{}</text></binding>\
         </visual><audio silent=\"true\"/></toast>",
        echapper(titre),
        echapper(texte)
    )
}

/// Un texte dans du XML : ses cinq caractères réservés échappés.
fn echapper(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
}

#[cfg(windows)]
mod imp {
    use std::path::Path;
    use std::sync::mpsc::{Receiver, Sender};

    use serde_json::Value;
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
    use windows::core::{HSTRING, IInspectable};

    use crate::chef::Evt;
    use crate::journal;

    /// Les notifications montrées qu'on garde, le temps qu'on puisse cliquer dessus : leur clic passe par elles.
    const GARDEES: usize = 20;

    /// Le fil : Eve s'inscrit (son icône, son nom, sous `id`), puis montre chaque notification reçue. Une notification
    /// que Windows refuse va au journal, une fois ; le fil continue.
    pub(super) fn mener(id: &str, nom: &str, icone: &Path, recus: &Receiver<(String, String)>, rapport: &Sender<Evt>) {
        if let Err(e) = inscrire(id, nom, icone) {
            journal::ecrire(&format!("notifications : Windows ne connaîtra pas Eve ({e})"));
        }
        let mut gardees = std::collections::VecDeque::new();
        let mut refus_dit = false;
        for (titre, texte) in recus {
            match montrer(id, &super::xml_de(&titre, &texte), rapport) {
                Ok(n) => {
                    gardees.push_back(n);
                    if gardees.len() > GARDEES {
                        gardees.pop_front();
                    }
                }
                Err(e) if !refus_dit => {
                    refus_dit = true;
                    journal::ecrire(&format!("notifications : Windows refuse d'en montrer une ({e})"));
                }
                Err(_) => {}
            }
        }
    }

    /// Eve auprès de Windows : son icône (le logo de sa fenêtre, en PNG, dans son dossier local) et son nom, sous son
    /// identifiant, dans `HKCU\Software\Classes\AppUserModelId`.
    fn inscrire(id: &str, nom: &str, icone: &Path) -> Result<(), String> {
        if let Some(png) = crate::natif::png_de_l_icone()
            && std::fs::read(icone).ok().as_deref() != Some(png.as_slice())
        {
            crate::reglages::ecrire_d_un_geste(icone, &png).map_err(|e| format!("son icône : {e}"))?;
        }
        let cle = format!(r"Software\Classes\AppUserModelId\{id}");
        crate::fenetre::ecrire_au_registre(&cle, "DisplayName", nom)?;
        crate::fenetre::ecrire_au_registre(&cle, "IconUri", &icone.display().to_string())
    }

    /// Montre une notification (son XML) sous l'identifiant d'Eve ; son clic rend le geste `notification` au chef.
    fn montrer(id: &str, xml: &str, rapport: &Sender<Evt>) -> windows::core::Result<ToastNotification> {
        let document = XmlDocument::new()?;
        document.LoadXml(&HSTRING::from(xml))?;
        let notification = ToastNotification::CreateToastNotification(&document)?;
        let rapport = rapport.clone();
        notification.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, _| {
            let _ = rapport.send(Evt::Ui { action: "notification".into(), valeur: Value::Null });
            Ok(())
        }))?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(id))?.Show(&notification)?;
        Ok(notification)
    }

    /// Combien de notifications de l'Eve `id` attendent dans le centre de notifications (l'essai le regarde).
    pub(crate) fn dans_le_centre(id: &str) -> windows::core::Result<u32> {
        ToastNotificationManager::History()?.GetHistoryWithId(&HSTRING::from(id))?.Size()
    }

    /// Retire du centre de notifications celles de l'Eve `id` (l'essai, après lui).
    pub(crate) fn vider_le_centre(id: &str) -> windows::core::Result<()> {
        ToastNotificationManager::History()?.ClearWithId(&HSTRING::from(id))
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::Path;
    use std::sync::mpsc::{Receiver, Sender};

    use crate::chef::Evt;

    pub(super) fn mener(_: &str, _: &str, _: &Path, recus: &Receiver<(String, String)>, _: &Sender<Evt>) {
        for _ in recus {}
    }
}

/// `eve --essai-notification` : une notification d'essai, montrée par le fil d'Eve sous l'identifiant du dossier local
/// de l'essai ; elle doit entrer au centre de notifications. Puis elle en est retirée, et l'inscription de l'essai
/// effacée du registre. Le code de sortie dit si elle y est entrée.
#[cfg(windows)]
pub fn essai(local: &Path) -> i32 {
    let id = identifiant(local);
    let (rapport, clics) = mpsc::channel();
    let n = Notifications::demarrer("Eve (essai)", local, rapport);
    n.montrer("Essai d'Eve", "Une notification d'essai : un clic ramènerait la fenêtre d'Eve.");
    let mut entree = 0;
    for _ in 0..40 {
        thread::sleep(std::time::Duration::from_millis(100));
        entree = imp::dans_le_centre(&id).unwrap_or(0);
        if entree > 0 {
            break;
        }
    }
    println!("notification d'essai sous {id} : {}", if entree > 0 { "au centre de notifications" } else { "absente" });
    thread::sleep(std::time::Duration::from_secs(4));
    if clics.try_recv().is_ok() {
        println!("et cliquée");
    }
    let _ = imp::vider_le_centre(&id);
    let cle = format!(r"Software\Classes\AppUserModelId\{id}");
    if let Err(e) = crate::fenetre::effacer_du_registre(&cle) {
        eprintln!("l'inscription de l'essai reste au registre : {e}");
    }
    i32::from(entree == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une notification : un identifiant par Eve, d'après son dossier local ; un texte coupé entre deux mots ; son XML,
    /// les caractères réservés échappés, sans son.
    #[test]
    fn une_notification_se_prepare() {
        assert_eq!(identifiant(Path::new(r"C:\x\specialistes\estimateur")), "ConstructoAI.Eve.estimateur");
        assert_eq!(identifiant(Path::new(r"C:\x\Eve d'essai")), "ConstructoAI.Eve.Eve-d-essai");
        assert_eq!(ecourter("Rappel :\nappeler   le notaire", 60), "Rappel : appeler le notaire");
        assert_eq!(ecourter("un deux trois quatre", 12), "un deux…");
        let x = xml_de("Rappel", "Tom & Jerry <3 « l'été »");
        assert!(x.contains("<text>Tom &amp; Jerry &lt;3 « l&apos;été »</text>"), "{x}");
        assert!(x.contains("<audio silent=\"true\"/>"), "{x}");
    }
}
