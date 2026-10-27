//! Les clés d'API qu'une Eve garde pour lire elle-même un service en ligne, pour le tableau de bord : celle de Render
//! (l'état de ses services et les erreurs de leurs journaux : `services.rs`) et celle de Stripe (ses montants :
//! `stripe.rs`). Chacune dans son fichier du dossier local, `render.cle` et `stripe.cle`, chiffrée par Windows pour la
//! session de la personne (DPAPI, comme `connexion.bin`) : lisible par ses seuls programmes, sur ce poste ; et la garde
//! refuse que Claude lise un fichier `.cle`. Une clé ne s'écrit nulle part ailleurs — ni au journal, ni dans
//! `reglages.json`, ni dans un message, qui n'en montre que les quatre derniers caractères.
//!
//! `eve --cle render` la pose, lue sur l'entrée standard (collée puis Entrée ; à la console, sans écho), jamais en
//! argument : une ligne de commande reste dans l'historique de PowerShell. `eve --cle render --retirer` la retire ;
//! `eve --cle` dit ce qui est posé. L'Eve qui tourne prend une clé à sa lecture suivante, sans redémarrer.

use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::connexion;
use crate::reglages::{dossier_local, ecrire_d_un_geste, sortie_de_commande};

/// L'« entropie » de DPAPI pour les clés : un autre programme de la session doit la connaître pour en déchiffrer une.
const ENTROPIE: &[u8] = b"Eve/cles/1";
/// Au-delà, ce n'est pas une clé d'API (celles de Stripe font une centaine de caractères).
const LONGUEUR_MAX: usize = 400;
/// Comment s'écrit la commande, quand elle est mal écrite.
const USAGE: &str = "eve --cle [render|stripe [--retirer]]";

/// Un service dont une Eve peut garder la clé.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Render,
    Stripe,
}

impl Service {
    pub const TOUS: [Service; 2] = [Service::Render, Service::Stripe];

    /// Son nom, tel qu'il s'écrit.
    pub fn nom(self) -> &'static str {
        match self {
            Service::Render => "Render",
            Service::Stripe => "Stripe",
        }
    }

    /// Son mot dans la commande (`eve --cle render`), qui nomme aussi son fichier (`render.cle`).
    fn mot(self) -> &'static str {
        match self {
            Service::Render => "render",
            Service::Stripe => "stripe",
        }
    }

    fn de(mot: &str) -> Option<Service> {
        Service::TOUS.into_iter().find(|s| s.mot() == mot.to_lowercase())
    }

    /// Les débuts d'une clé de ce service : une clé collée pour l'autre service se voit tout de suite. Pour Stripe, une
    /// clé restreinte (`rk_`, que la personne crée en lecture seule) ou la clé secrète (`sk_`, tous les droits).
    fn debuts(self) -> &'static [&'static str] {
        match self {
            Service::Render => &["rnd_"],
            Service::Stripe => &["rk_live_", "rk_test_", "sk_live_", "sk_test_"],
        }
    }
}

/// Le fichier de la clé de ce service, dans ce dossier local.
pub fn chemin_dans(dossier: &Path, service: Service) -> PathBuf {
    dossier.join(format!("{}.cle", service.mot()))
}

/// Le fichier de la clé de ce service, dans le dossier local de cette Eve : sa date dit qu'une clé a changé.
pub fn chemin(service: Service) -> PathBuf {
    chemin_dans(&dossier_local(), service)
}

/// La clé de ce service, si elle est posée et se lit ; rien sinon (absente, chiffrée pour une autre session, abîmée).
pub fn lire(service: Service) -> Option<String> {
    lire_dans(&dossier_local(), service)
}

/// La clé de ce service dans ce dossier local.
pub fn lire_dans(dossier: &Path, service: Service) -> Option<String> {
    let chiffre = fs::read(chemin_dans(dossier, service)).ok()?;
    let clair = connexion::proteger(&chiffre, false, ENTROPIE, "").ok()?;
    String::from_utf8(clair).ok().filter(|c| valable(service, c).is_ok())
}

/// Pose cette clé dans ce dossier local, vérifiée d'abord ([`valable`]), chiffrée, d'un seul geste.
pub fn poser_dans(dossier: &Path, service: Service, cle: &str) -> Result<(), String> {
    valable(service, cle)?;
    let chiffre = connexion::proteger(cle.as_bytes(), true, ENTROPIE, "Eve - clé d'API")?;
    ecrire_d_un_geste(&chemin_dans(dossier, service), &chiffre).map_err(|e| format!("la clé ne s'écrit pas ({e})"))
}

/// Retire la clé de ce service de ce dossier local ; vrai s'il y en avait une.
pub fn retirer_dans(dossier: &Path, service: Service) -> Result<bool, String> {
    match fs::remove_file(chemin_dans(dossier, service)) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("la clé ne se retire pas ({e})")),
    }
}

/// Une clé de ce service en a-t-elle la forme ? Des caractères visibles seulement (un retour de ligne glisserait un
/// en-tête de plus dans une requête), pas trop longue, et le bon début ; la clé publique de Stripe (`pk_`), qui ne lit
/// rien, se dit pour ce qu'elle est.
pub fn valable(service: Service, cle: &str) -> Result<(), String> {
    if cle.is_empty() {
        return Err("La clé est vide.".into());
    }
    if cle.len() > LONGUEUR_MAX || !cle.chars().all(|c| c.is_ascii_graphic()) {
        return Err("Ce n'est pas une clé d'API : trop longue, ou des caractères qui n'en sont pas.".into());
    }
    if service == Service::Stripe && cle.starts_with("pk_") {
        return Err(
            "C'est la clé publique de Stripe (pk_…) : il faut une clé restreinte (rk_…), en lecture seule.".into()
        );
    }
    if !service.debuts().iter().any(|d| cle.starts_with(d)) {
        return Err(format!("Une clé de {} commence par {}.", service.nom(), service.debuts().join(", ")));
    }
    Ok(())
}

/// Ce qu'on montre d'une clé : ses quatre derniers caractères (« …XqC »).
pub fn fin_de(cle: &str) -> String {
    let n = cle.chars().count();
    format!("…{}", cle.chars().skip(n.saturating_sub(4)).collect::<String>())
}

// ---- La commande -------------------------------------------------------------------------------------------------

/// `eve --cle [render|stripe [--retirer]]`, sur le dossier local de cette Eve (`EVE_DOSSIER_LOCAL`) : ni le journal ni
/// la vitrine ne s'ouvrent, comme les commandes des réglages. Rien d'autre : `None`.
pub fn commande(args: &[String]) -> Option<i32> {
    let (premier, reste) = args.split_first()?;
    if premier != "--cle" {
        return None;
    }
    let mots: Vec<&str> = reste.iter().map(String::as_str).collect();
    let dossier = dossier_local();
    let fait = match mots.as_slice() {
        [] => Ok(etat_des_cles(&dossier)),
        [mot] => match Service::de(mot) {
            Some(s) => lire_la_cle(s).and_then(|cle| {
                poser_dans(&dossier, s, &cle)?;
                Ok(cle_posee(s, &cle))
            }),
            None => Err(USAGE.to_string()),
        },
        [mot, "--retirer"] => match Service::de(mot) {
            Some(s) => retirer_dans(&dossier, s).map(|il_y_en_avait| match il_y_en_avait {
                true => format!("La clé de {} est retirée : Eve ne lit plus ce service.", s.nom()),
                false => format!("Aucune clé de {} n'était posée.", s.nom()),
            }),
            None => Err(USAGE.to_string()),
        },
        _ => Err(USAGE.to_string()),
    };
    Some(sortie_de_commande(fait))
}

/// Ce qui est posé dans ce dossier local, un service par ligne.
fn etat_des_cles(dossier: &Path) -> String {
    let lignes: Vec<String> = Service::TOUS
        .iter()
        .map(|&s| match lire_dans(dossier, s) {
            Some(cle) => format!("  {} : une clé posée ({})", s.nom(), fin_de(&cle)),
            None if chemin_dans(dossier, s).exists() => {
                format!("  {} : une clé posée, mais illisible ici (une autre session de Windows ?)", s.nom())
            }
            None => format!("  {} : aucune clé", s.nom()),
        })
        .collect();
    format!("Les clés d'API de cette Eve (dans {}) :\n{}", dossier.display(), lignes.join("\n"))
}

/// Ce que dit la pose d'une clé ; pour une clé secrète de Stripe, qu'une clé restreinte suffit.
fn cle_posee(s: Service, cle: &str) -> String {
    let mut m = format!("La clé de {} est posée ({}) : Eve la prend à sa lecture suivante.", s.nom(), fin_de(cle));
    if s == Service::Stripe && cle.starts_with("sk_") {
        m.push_str(
            "\nC'est la clé secrète : elle donne tous les droits sur le compte. Une clé restreinte en lecture seule \
             (rk_…, dans le tableau de bord de Stripe : Développeurs, Clés API) suffit à Eve.",
        );
    }
    m
}

/// La clé, lue sur l'entrée standard : à la console, une invite, et sans écho (collée, elle ne s'affiche pas) ; d'un
/// tube (`… | eve --cle stripe`), sa première ligne. Ses blancs autour s'en vont ([`cle_de_la_ligne`]).
fn lire_la_cle(s: Service) -> Result<String, String> {
    let entree = std::io::stdin();
    let console = entree.is_terminal();
    if console {
        eprint!("Colle la clé d'API de {}, puis Entrée (elle ne s'affiche pas) : ", s.nom());
        let _ = std::io::stderr().flush();
    }
    let _sans_echo = console.then(imp::sans_echo);
    let mut ligne = String::new();
    entree.lock().read_line(&mut ligne).map_err(|e| format!("La clé ne se lit pas ({e})."))?;
    if console {
        eprintln!();
    }
    Ok(cle_de_la_ligne(&ligne).to_string())
}

/// La clé d'une ligne lue : ses blancs autour ôtés, et la marque d'octets (U+FEFF) que Windows PowerShell 5.1 met en
/// tête de ce qu'il passe à un programme par un tube (`Get-Clipboard | eve --cle stripe`) ; sans cela, la clé ainsi
/// collée était refusée (« Ce n'est pas une clé d'API »).
fn cle_de_la_ligne(ligne: &str) -> &str {
    ligne.trim().trim_start_matches('\u{feff}').trim()
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        CONSOLE_MODE, ENABLE_ECHO_INPUT, GetConsoleMode, GetStdHandle, STD_INPUT_HANDLE, SetConsoleMode,
    };

    /// La console sans écho, tant qu'il vit : son mode d'avant revient avec lui.
    pub struct SansEcho(Option<(HANDLE, CONSOLE_MODE)>);

    impl Drop for SansEcho {
        fn drop(&mut self) {
            if let Some((h, mode)) = self.0 {
                // SAFETY : le handle d'entrée de la console du processus, remis dans son mode d'avant.
                unsafe {
                    let _ = SetConsoleMode(h, mode);
                }
            }
        }
    }

    pub fn sans_echo() -> SansEcho {
        // SAFETY : le handle d'entrée de la console du processus, lu puis réglé ; sans console, les appels échouent
        // sans effet, et rien n'est à remettre.
        unsafe {
            let Ok(h) = GetStdHandle(STD_INPUT_HANDLE) else { return SansEcho(None) };
            let mut mode = CONSOLE_MODE::default();
            if GetConsoleMode(h, &mut mode).is_err() || SetConsoleMode(h, mode & !ENABLE_ECHO_INPUT).is_err() {
                return SansEcho(None);
            }
            SansEcho(Some((h, mode)))
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn sans_echo() {}
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn une_cle_se_reconnait_a_sa_forme() {
        assert!(valable(Service::Render, "rnd_AbCdEf123").is_ok());
        assert!(valable(Service::Stripe, "rk_live_51Exemple").is_ok());
        assert!(valable(Service::Stripe, "sk_test_51Exemple").is_ok());
        assert!(valable(Service::Render, "").is_err());
        assert!(valable(Service::Render, "rnd_ab cd").is_err());
        assert!(valable(Service::Render, "rnd_abc\r\nX-Autre: 1").is_err());
        // Une clé collée pour l'autre service, ou la clé publique de Stripe.
        assert!(valable(Service::Render, "rk_live_51Exemple").is_err());
        assert!(valable(Service::Stripe, "rnd_AbCdEf123").is_err());
        assert!(valable(Service::Stripe, "pk_live_51Exemple").unwrap_err().contains("publique"));
    }

    #[test]
    fn une_cle_venue_d_un_tube_de_powershell() {
        assert_eq!(cle_de_la_ligne("\u{feff}rk_live_51Exemple\r\n"), "rk_live_51Exemple");
        assert_eq!(cle_de_la_ligne("  rk_live_51Exemple \n"), "rk_live_51Exemple");
        assert!(valable(Service::Stripe, cle_de_la_ligne("\u{feff}sk_test_51Exemple\r\n")).is_ok());
    }

    #[test]
    fn d_une_cle_on_ne_montre_que_sa_fin() {
        assert_eq!(fin_de("rnd_AbCdEf123"), "…f123");
        assert_eq!(fin_de("abc"), "…abc");
    }

    #[test]
    fn les_mots_de_la_commande() {
        assert_eq!(Service::de("render"), Some(Service::Render));
        assert_eq!(Service::de("Stripe"), Some(Service::Stripe));
        assert_eq!(Service::de("vapi"), None);
        assert!(commande(&["--profils".to_string()]).is_none());
        let fichier = chemin_dans(Path::new(r"C:\x"), Service::Stripe);
        assert!(fichier.ends_with("stripe.cle"));
    }

    #[cfg(windows)]
    #[test]
    fn une_cle_posee_se_relit_chiffree() {
        let dossier = std::env::temp_dir().join(format!("eve-essai-cles-{}", std::process::id()));
        fs::create_dir_all(&dossier).unwrap();
        assert_eq!(lire_dans(&dossier, Service::Render), None);
        poser_dans(&dossier, Service::Render, "rnd_EssaiInvente42").unwrap();
        assert_eq!(lire_dans(&dossier, Service::Render).as_deref(), Some("rnd_EssaiInvente42"));
        // Le fichier ne porte pas la clé en clair.
        let brut = fs::read(chemin_dans(&dossier, Service::Render)).unwrap();
        assert!(!brut.windows(8).any(|w| w == b"EssaiInv"));
        // Une clé de forme fausse ne s'écrit pas, et l'ancienne reste.
        assert!(poser_dans(&dossier, Service::Render, "pas une clé").is_err());
        assert_eq!(lire_dans(&dossier, Service::Render).as_deref(), Some("rnd_EssaiInvente42"));
        assert!(retirer_dans(&dossier, Service::Render).unwrap());
        assert!(!retirer_dans(&dossier, Service::Render).unwrap());
        assert_eq!(lire_dans(&dossier, Service::Render), None);
        let _ = fs::remove_dir_all(&dossier);
    }
}
