//! Une lecture HTTPS sans dépendance : WinHTTP, la pile de Windows (le proxy du poste, ses certificats). Pour le
//! calendrier en ligne, avec le jeton d'Adjointe en « Bearer » — jamais écrit, jamais journalisé, et jamais renvoyé
//! ailleurs : une redirection n'est pas suivie (elle rend son code 3xx), comme dans les scripts de l'Adjointe, où une redirection
//! suivie aurait porté l'en-tête `Authorization` à l'adresse de destination, quelle qu'elle soit.
//!
//! La météo du centre de commande (`meteo.rs`) passe par `get_public` : sans jeton, sous un nom neutre — le logiciel
//! et sa version ; un service public n'a pas à savoir pour qui Eve demande —, et sans corps compressé. La connexion à
//! Constructo AI (`connexion.rs` : l'enregistrement d'Eve, l'échange du code, le renouvellement des jetons) passe par
//! `post_formulaire` et `post_json`, sous le même nom neutre.
//!
//! `https://` seulement — sauf `http://127.0.0.1` et `http://localhost`, qui ne quittent pas le poste : les essais de la
//! connexion y montent un faux serveur.

use std::time::Duration;

/// Le plus gros corps accepté.
const CORPS_MAX: usize = 20 * 1024 * 1024;
/// Qui demande : le serveur le voit dans ses journaux.
const AGENT: &str = "Eve/1.0 (Entrepreneur Général)";
/// Qui demande à un service public : le logiciel et sa version, rien de la personne ni de l'entreprise.
const AGENT_PUBLIC: &str = concat!("Eve/", env!("CARGO_PKG_VERSION"));

/// `GET url` ; rend le code HTTP et le corps. `jeton` : l'en-tête `Authorization: Bearer …`.
pub fn get(url: &str, jeton: Option<&str>, delai: Duration) -> Result<(u16, Vec<u8>), String> {
    let adresse = Adresse::lire(url)?;
    // Un jeton d'Adjointe n'a que des caractères visibles : un retour de ligne y glisserait un autre en-tête.
    if let Some(j) = jeton
        && (j.is_empty() || !j.bytes().all(|b| b.is_ascii_graphic()))
    {
        return Err("le jeton n'a pas la forme d'un jeton : rien n'est envoyé".into());
    }
    winhttp::envoyer("GET", &adresse, AGENT, jeton, None, true, delai)
}

/// `GET url` à un service public (la météo) : sans jeton, sous le nom `AGENT_PUBLIC`, sans suivre de redirection ;
/// rend le code HTTP et le corps. Et sans compression : à qui offre « gzip, deflate », open-meteo répond en deflate
/// (zlib), et WinHTTP en abandonne la lecture (E_ABORT, vu le 24 octobre 2026) ; un bulletin fait moins d'un ko.
pub fn get_public(url: &str, delai: Duration) -> Result<(u16, Vec<u8>), String> {
    winhttp::envoyer("GET", &Adresse::lire(url)?, AGENT_PUBLIC, None, None, false, delai)
}

/// `POST url` d'un formulaire (`application/x-www-form-urlencoded`) : l'échange d'un code OAuth, un renouvellement.
/// Sous le nom neutre, sans compression (une réponse de quelques centaines d'octets), sans suivre de redirection.
pub fn post_formulaire(url: &str, champs: &[(&str, &str)], delai: Duration) -> Result<(u16, Vec<u8>), String> {
    let corps = champs.iter().map(|(n, v)| format!("{}={}", encoder(n), encoder(v))).collect::<Vec<_>>().join("&");
    let corps = Some(("application/x-www-form-urlencoded", corps.as_bytes()));
    winhttp::envoyer("POST", &Adresse::lire(url)?, AGENT_PUBLIC, None, corps, false, delai)
}

/// `POST url` d'un objet JSON : l'enregistrement d'Eve auprès du serveur OAuth.
pub fn post_json(url: &str, corps: &serde_json::Value, delai: Duration) -> Result<(u16, Vec<u8>), String> {
    let octets = corps.to_string().into_bytes();
    winhttp::envoyer(
        "POST",
        &Adresse::lire(url)?,
        AGENT_PUBLIC,
        None,
        Some(("application/json", &octets)),
        false,
        delai,
    )
}

/// Le codage d'une valeur de formulaire ou d'adresse (`application/x-www-form-urlencoded`, RFC 3986 : les caractères
/// non réservés passent, tout le reste en `%XX`).
pub fn encoder(s: &str) -> String {
    let mut sortie = String::with_capacity(s.len() * 3);
    for o in s.bytes() {
        if o.is_ascii_alphanumeric() || matches!(o, b'-' | b'.' | b'_' | b'~') {
            sortie.push(char::from(o));
        } else {
            sortie.push_str(&format!("%{o:02X}"));
        }
    }
    sortie
}

/// Une adresse `https://hôte[:port]/chemin?requête`, découpée pour WinHTTP.
#[derive(Debug, PartialEq, Eq)]
struct Adresse {
    hote: String,
    port: u16,
    /// Le chemin et la requête (« /api/calendrier?donnees=1 »), sans le fragment.
    chemin: String,
    /// `https` : toujours, sauf vers le poste lui-même.
    securise: bool,
}

impl Adresse {
    fn lire(url: &str) -> Result<Adresse, String> {
        let url = url.trim();
        let (reste, securise) = match (url.get(..8), url.get(..7)) {
            (Some(debut), _) if debut.eq_ignore_ascii_case("https://") => (&url[8..], true),
            (_, Some(debut)) if debut.eq_ignore_ascii_case("http://") => (&url[7..], false),
            _ => return Err("adresse refusée : https seulement".into()),
        };
        let fin = reste.find(['/', '?', '#']).unwrap_or(reste.len());
        let (autorite, suite) = reste.split_at(fin);
        let suite = suite.split('#').next().unwrap_or_default();
        let chemin = if suite.starts_with('/') { suite.to_string() } else { format!("/{suite}") };
        if chemin.chars().any(char::is_control) {
            return Err("adresse refusée : caractère de contrôle dans le chemin".into());
        }
        if autorite.contains('@') {
            return Err("adresse refusée : pas d'identifiant dans l'adresse".into());
        }
        let (hote, port) = match autorite.rsplit_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().ok().filter(|&p| p > 0).ok_or("adresse refusée : port illisible")?),
            None => (autorite, if securise { 443 } else { 80 }),
        };
        let lisible = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '.';
        if hote.is_empty() || !hote.chars().all(lisible) {
            return Err("adresse refusée : nom de serveur illisible".into());
        }
        let hote = hote.to_ascii_lowercase();
        // En clair, seulement vers le poste lui-même : rien ne traverse le réseau sans chiffrement.
        if !securise && hote != "127.0.0.1" && hote != "localhost" {
            return Err("adresse refusée : https seulement".into());
        }
        Ok(Adresse { hote, port, chemin, securise })
    }
}

/// Efface une copie du jeton dès qu'elle ne sert plus.
#[cfg_attr(not(windows), allow(dead_code))]
fn effacer(v: &mut [u16]) {
    for c in v.iter_mut() {
        // SAFETY : `c` est une référence exclusive et valide sur un élément du tableau.
        unsafe { std::ptr::write_volatile(c, 0) };
    }
}

#[cfg(windows)]
mod winhttp {
    use std::ffi::c_void;
    use std::time::Duration;

    use windows::Win32::Networking::WinHttp::*;
    use windows::core::PCWSTR;

    use super::{Adresse, CORPS_MAX, effacer};

    /// Un handle WinHTTP (session, connexion, requête), fermé à la sortie quoi qu'il arrive.
    struct Poignee(*mut c_void);

    impl Poignee {
        /// Le handle que vient de rendre WinHTTP ; nul : l'erreur du fil, lue aussitôt.
        fn ou_erreur(h: *mut c_void, etape: &str) -> Result<Poignee, String> {
            if h.is_null() {
                let code = std::io::Error::last_os_error().raw_os_error().unwrap_or(0) as u32;
                Err(erreur(etape, code))
            } else {
                Ok(Poignee(h))
            }
        }
    }

    impl Drop for Poignee {
        fn drop(&mut self) {
            // SAFETY : le handle vient de WinHTTP, n'a pas encore été fermé, et plus rien ne s'en sert après.
            let _ = unsafe { WinHttpCloseHandle(self.0) };
        }
    }

    /// Une chaîne pour Windows : UTF-16, terminée par un zéro.
    fn large(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Le code Win32 d'une erreur rendue par la crate (un HRESULT 0x8007xxxx).
    fn code(e: &windows::core::Error) -> u32 {
        let hr = e.code().0 as u32;
        if hr & 0xFFFF_0000 == 0x8007_0000 { hr & 0xFFFF } else { hr }
    }

    /// Les erreurs de WinHTTP, en français (leurs messages ne sont pas dans la table du système).
    fn erreur(etape: &str, code: u32) -> String {
        let quoi = match code {
            ERROR_WINHTTP_TIMEOUT => "le serveur ne répond pas à temps",
            ERROR_WINHTTP_NAME_NOT_RESOLVED => "le nom du serveur est introuvable (pas de réseau ?)",
            ERROR_WINHTTP_CANNOT_CONNECT => "la connexion au serveur est impossible",
            ERROR_WINHTTP_CONNECTION_ERROR => "la connexion a été coupée",
            ERROR_WINHTTP_SECURE_FAILURE
            | ERROR_WINHTTP_SECURE_CERT_CN_INVALID
            | ERROR_WINHTTP_SECURE_CERT_DATE_INVALID
            | ERROR_WINHTTP_SECURE_CERT_REVOKED
            | ERROR_WINHTTP_SECURE_CERT_REV_FAILED
            | ERROR_WINHTTP_SECURE_CERT_WRONG_USAGE => "la connexion sécurisée est refusée (certificat)",
            ERROR_WINHTTP_INVALID_SERVER_RESPONSE => "la réponse du serveur est illisible",
            ERROR_WINHTTP_AUTO_PROXY_SERVICE_ERROR
            | ERROR_WINHTTP_AUTODETECTION_FAILED
            | ERROR_WINHTTP_BAD_AUTO_PROXY_SCRIPT
            | ERROR_WINHTTP_UNABLE_TO_DOWNLOAD_SCRIPT => "le proxy du poste ne répond pas",
            ERROR_WINHTTP_OPERATION_CANCELLED => "la demande a été annulée",
            ERROR_WINHTTP_INVALID_URL | ERROR_WINHTTP_UNRECOGNIZED_SCHEME => "l'adresse est refusée",
            _ => "",
        };
        if quoi.is_empty() {
            format!("WinHTTP, {etape} : erreur {code}")
        } else {
            format!("{quoi} (WinHTTP {code}, {etape})")
        }
    }

    /// Une requête : `verbe`, et un `corps` (son type et ses octets) pour un POST.
    pub fn envoyer(
        verbe: &str,
        a: &Adresse,
        agent: &str,
        jeton: Option<&str>,
        corps: Option<(&str, &[u8])>,
        compression: bool,
        delai: Duration,
    ) -> Result<(u16, Vec<u8>), String> {
        let agent = large(agent);
        // SAFETY : l'agent est une chaîne terminée par un zéro qui vit jusqu'au retour ; le proxy automatique ne prend
        // ni nom de proxy ni liste d'exceptions (deux pointeurs nuls) ; le handle rendu est gardé par `Poignee`.
        let session = Poignee::ou_erreur(
            unsafe {
                WinHttpOpen(
                    PCWSTR(agent.as_ptr()),
                    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                    PCWSTR::null(),
                    PCWSTR::null(),
                    0,
                )
            },
            "ouverture",
        )?;
        let ms = i32::try_from(delai.as_millis()).unwrap_or(i32::MAX).max(1);
        // SAFETY : `session` est un handle de session ouvert.
        unsafe { WinHttpSetTimeouts(session.0, ms, ms, ms, ms) }.map_err(|e| erreur("délais", code(&e)))?;

        let hote = large(&a.hote);
        // SAFETY : `session` est ouvert, `hote` est terminé par un zéro et vit jusqu'au retour.
        let connexion =
            Poignee::ou_erreur(unsafe { WinHttpConnect(session.0, PCWSTR(hote.as_ptr()), a.port, 0) }, "connexion")?;

        let verbe = large(verbe);
        let objet = large(&a.chemin);
        // En clair seulement vers le poste lui-même (`Adresse::lire` le garantit).
        let drapeaux = if a.securise { WINHTTP_FLAG_SECURE } else { WINHTTP_OPEN_REQUEST_FLAGS(0) };
        // SAFETY : `connexion` est ouverte ; le verbe et le chemin sont terminés par un zéro ; la version, le référent et
        // les types acceptés par défaut sont des pointeurs nuls, permis par WinHTTP.
        let requete = Poignee::ou_erreur(
            unsafe {
                WinHttpOpenRequest(
                    connexion.0,
                    PCWSTR(verbe.as_ptr()),
                    PCWSTR(objet.as_ptr()),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    std::ptr::null(),
                    drapeaux,
                )
            },
            "requête",
        )?;

        // Aucune redirection : le jeton n'irait nulle part ailleurs ; un 3xx revient tel quel à l'appelant.
        let jamais = WINHTTP_OPTION_REDIRECT_POLICY_NEVER.to_ne_bytes();
        // SAFETY : `requete` est ouverte ; l'option attend un DWORD, que porte `jamais` pendant l'appel.
        unsafe { WinHttpSetOption(Some(requete.0.cast_const()), WINHTTP_OPTION_REDIRECT_POLICY, Some(&jamais)) }
            .map_err(|e| erreur("redirections", code(&e)))?;
        // Le corps compressé, quand le serveur le veut bien (un essai : sans elle, la lecture se fait quand même).
        if compression {
            let formes = (WINHTTP_DECOMPRESSION_FLAG_GZIP | WINHTTP_DECOMPRESSION_FLAG_DEFLATE).to_ne_bytes();
            // SAFETY : comme ci-dessus, un DWORD le temps de l'appel.
            let _ =
                unsafe { WinHttpSetOption(Some(requete.0.cast_const()), WINHTTP_OPTION_DECOMPRESSION, Some(&formes)) };
        }

        let ajout = WINHTTP_ADDREQ_FLAG_ADD | WINHTTP_ADDREQ_FLAG_REPLACE;
        let entetes: Vec<u16> = "Accept: application/json\r\nCache-Control: no-cache\r\n".encode_utf16().collect();
        // SAFETY : `requete` est ouverte ; la longueur des en-têtes est passée avec eux (pas de zéro final requis).
        unsafe { WinHttpAddRequestHeaders(requete.0, &entetes, ajout) }.map_err(|e| erreur("en-têtes", code(&e)))?;
        if let Some(j) = jeton {
            let mut acces: Vec<u16> = "Authorization: Bearer ".encode_utf16().chain(j.encode_utf16()).collect();
            // SAFETY : comme ci-dessus.
            let r = unsafe { WinHttpAddRequestHeaders(requete.0, &acces, ajout) };
            effacer(&mut acces);
            r.map_err(|e| erreur("en-tête d'accès", code(&e)))?;
        }

        match corps {
            None => {
                // SAFETY : `requete` est ouverte ; ni en-tête de plus ni corps (une lecture).
                unsafe { WinHttpSendRequest(requete.0, None, None, 0, 0, 0) }.map_err(|e| erreur("envoi", code(&e)))?;
            }
            Some((sorte, octets)) => {
                let type_corps: Vec<u16> = format!("Content-Type: {sorte}\r\n").encode_utf16().collect();
                // SAFETY : comme pour les en-têtes ci-dessus.
                unsafe { WinHttpAddRequestHeaders(requete.0, &type_corps, ajout) }
                    .map_err(|e| erreur("en-tête du corps", code(&e)))?;
                let n = u32::try_from(octets.len()).map_err(|_| "le corps est trop gros".to_string())?;
                // SAFETY : `requete` est ouverte ; le corps (`n` octets) vit pendant l'appel, qui l'envoie en entier
                // (longueur totale = longueur fournie) ; aucun contexte.
                unsafe { WinHttpSendRequest(requete.0, None, Some(octets.as_ptr().cast()), n, n, 0) }
                    .map_err(|e| erreur("envoi", code(&e)))?;
            }
        }
        // SAFETY : `requete` vient d'être envoyée ; le paramètre réservé est nul.
        unsafe { WinHttpReceiveResponse(requete.0, std::ptr::null_mut()) }.map_err(|e| erreur("réponse", code(&e)))?;

        let mut statut: u32 = 0;
        let mut taille = std::mem::size_of::<u32>() as u32;
        // SAFETY : `statut` est un DWORD de `taille` octets qui vit pendant l'appel ; le code se lit par son numéro
        // (pas de nom d'en-tête, pas d'index).
        unsafe {
            WinHttpQueryHeaders(
                requete.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some((&raw mut statut).cast()),
                &mut taille,
                std::ptr::null_mut(),
            )
        }
        .map_err(|e| erreur("code de la réponse", code(&e)))?;

        let mut corps: Vec<u8> = Vec::new();
        loop {
            let mut dispo: u32 = 0;
            // SAFETY : `requete` a reçu sa réponse ; `dispo` vit pendant l'appel.
            unsafe { WinHttpQueryDataAvailable(requete.0, &mut dispo) }.map_err(|e| erreur("lecture", code(&e)))?;
            if dispo == 0 {
                break;
            }
            if corps.len() + dispo as usize > CORPS_MAX {
                return Err(format!("la réponse dépasse {} Mo", CORPS_MAX / (1024 * 1024)));
            }
            let debut = corps.len();
            corps.resize(debut + dispo as usize, 0);
            let mut lus: u32 = 0;
            // SAFETY : la place réservée fait `dispo` octets à partir de `debut` ; `lus` vit pendant l'appel.
            unsafe { WinHttpReadData(requete.0, corps[debut..].as_mut_ptr().cast(), dispo, &mut lus) }
                .map_err(|e| erreur("lecture", code(&e)))?;
            corps.truncate(debut + lus as usize);
            if lus == 0 {
                break;
            }
        }
        Ok((u16::try_from(statut).unwrap_or(0), corps))
    }
}

#[cfg(not(windows))]
mod winhttp {
    use std::time::Duration;

    use super::Adresse;

    pub fn envoyer(
        _verbe: &str,
        _a: &Adresse,
        _agent: &str,
        _jeton: Option<&str>,
        _corps: Option<(&str, &[u8])>,
        _compression: bool,
        _delai: Duration,
    ) -> Result<(u16, Vec<u8>), String> {
        Err("le HTTPS passe par WinHTTP : Windows seulement".into())
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    fn adresse(url: &str) -> Adresse {
        Adresse::lire(url).unwrap_or_else(|e| panic!("{url} : {e}"))
    }

    #[test]
    fn une_adresse_se_decoupe() {
        let a = adresse("https://entrepreneur-general.ca/api/calendrier?donnees=1");
        assert_eq!(
            a,
            Adresse {
                hote: "entrepreneur-general.ca".into(),
                port: 443,
                chemin: "/api/calendrier?donnees=1".into(),
                securise: true
            }
        );
        assert_eq!(adresse("HTTPS://Exemple.CA").chemin, "/");
        assert_eq!(adresse("https://exemple.ca?x=1").chemin, "/?x=1");
        let b = adresse("https://exemple.ca:8443/a/b#fragment");
        assert_eq!((b.port, b.chemin.as_str()), (8443, "/a/b"));
    }

    /// En clair, seulement vers le poste lui-même (le faux serveur des essais de la connexion à Constructo AI).
    #[test]
    fn le_clair_ne_vise_que_le_poste() {
        let a = adresse("http://127.0.0.1:47997/token");
        assert_eq!((a.hote.as_str(), a.port, a.chemin.as_str(), a.securise), ("127.0.0.1", 47997, "/token", false));
        assert_eq!(adresse("HTTP://LocalHost/x").port, 80);
        for url in ["http://127.0.0.2/", "http://localhost.exemple.ca/", "http://exemple.ca:80/", "http://0x7f000001/"]
        {
            assert!(Adresse::lire(url).is_err(), "{url}");
        }
    }

    #[test]
    fn un_formulaire_se_code() {
        assert_eq!(encoder("abc-._~XYZ09"), "abc-._~XYZ09");
        assert_eq!(encoder("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(encoder("http://127.0.0.1:47811/retour"), "http%3A%2F%2F127.0.0.1%3A47811%2Fretour");
    }

    #[test]
    fn une_adresse_douteuse_est_refusee() {
        for url in [
            "http://exemple.ca/", "ftp://exemple.ca/", "https://", "https://moi@exemple.ca/", "https://exemple.ca:0/",
            "https://exemple.ca:abc/", "https://exem ple.ca/", "https://exemple.ca/a\r\nb",
        ] {
            assert!(Adresse::lire(url).is_err(), "{url}");
        }
    }

    /// Un jeton qui glisserait un en-tête est refusé avant tout envoi, et l'erreur ne le répète pas.
    #[test]
    fn un_jeton_mal_forme_ne_part_pas() {
        let jeton = "eg_essai\r\nX-Autre: 1";
        let e = get("https://exemple.invalid/", Some(jeton), Duration::from_secs(1)).unwrap_err();
        assert!(!e.contains("eg_essai") && !e.contains("X-Autre"), "{e}");
        assert!(get("https://exemple.invalid/", Some(""), Duration::from_secs(1)).is_err());
    }

    /// Un service public ne reçoit que le nom du logiciel et sa version : ni personne, ni entreprise, ni adresse.
    #[test]
    fn le_nom_public_ne_dit_rien_de_personne() {
        assert!(AGENT_PUBLIC.starts_with("Eve/"), "{AGENT_PUBLIC}");
        assert!(AGENT_PUBLIC.bytes().all(|b| b.is_ascii_alphanumeric() || b"/.-".contains(&b)), "{AGENT_PUBLIC}");
        assert!(get_public("http://api.open-meteo.com/v1/forecast", Duration::from_secs(1)).is_err());
    }

    #[test]
    fn le_jeton_s_efface() {
        let mut v: Vec<u16> = "eg_abc".encode_utf16().collect();
        effacer(&mut v);
        assert!(v.iter().all(|&c| c == 0));
    }
}
