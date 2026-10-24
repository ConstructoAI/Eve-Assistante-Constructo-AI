//! La connexion à Constructo AI : Eve se relie au serveur MCP de l'ERP comme le fait claude.ai — OAuth 2.1 avec PKCE,
//! sans secret (`mcp_oauth.py` du serveur : un client public, `token_endpoint_auth_method = none`). Portée de l'édition
//! Constructo d'Eve (son `connexion.rs`), pour tout spécialiste qui a le privilège `constructo` :
//! chacun a sa connexion, à son compte, dans son dossier local. La page de connexion de Constructo AI (le courriel et le
//! mot de passe de l'entreprise) s'ouvre dans le navigateur du poste, et revient sur la vitrine de cette Eve
//! (`http://127.0.0.1:<port>/connexion/retour` : une adresse de bouclage, que le serveur accepte).
//!
//! Ce qu'Eve garde ([`Etat`]) : son identifiant de client (enregistré une fois par port), le jeton d'accès (une heure)
//! et celui de renouvellement (trente jours). Celui-ci CHANGE à chaque renouvellement, et le serveur révoque toute la
//! famille si un ancien resservait : il s'écrit donc sur le disque avant de servir à quoi que ce soit. Le tout dans
//! `connexion.bin`, chiffré par Windows pour la session de l'utilisateur (DPAPI) : lisible par ses seuls programmes,
//! sur ce poste — et la garde refuse que Claude le lise.
//!
//! Le jeton d'accès ne passe ni sur une ligne de commande ni dans l'environnement de Claude Code : Eve le lui remet par
//! la demande de contrôle `mcp_set_servers` ([`Lien::serveurs_mcp`]), et le remplace de la même façon quand il se
//! renouvelle, sans relancer la conversation (mesuré par l'édition Constructo le 22 octobre 2026, sur un faux serveur).
//!
//! `EVE_CONSTRUCTO_MCP` nomme un autre serveur que celui de Constructo AI : les essais, sur le faux que monte
//! `eve --essai-constructo`. Les essais ci-dessous en montent un dans le programme même : jamais le vrai.

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eve::garde::{SERVEUR_CONSTRUCTO, ecourter};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::https;
use crate::reglages::{dossier_local, ecrire_d_un_geste};

/// Le serveur MCP de Constructo AI (`McpPage.tsx` de l'ERP : l'adresse du connecteur, sans `/mcp`).
pub const SERVEUR_PAR_DEFAUT: &str = "https://constructo-mcp.onrender.com";
/// Le nom sous lequel Eve s'enregistre : le serveur le garde dans ses journaux. Rien de la personne.
const NOM_CLIENT: &str = "Eve de Constructo AI (assistante vocale)";
/// La portée unique du serveur.
const PORTEE: &str = "mcp";
/// Le chemin du retour sur la vitrine.
pub const CHEMIN_RETOUR: &str = "/connexion/retour";
/// Le temps laissé pour se connecter dans le navigateur (le code du serveur vit dix minutes).
pub const DELAI_CONNEXION: Duration = Duration::from_secs(10 * 60);
/// Le jeton d'accès se renouvelle quand il lui reste moins que ceci.
const MARGE: u64 = 10 * 60;
/// Une requête au serveur : Render réveille un service endormi en une trentaine de secondes.
const DELAI_HTTP: Duration = Duration::from_secs(45);
/// Le fichier chiffré, dans le dossier local.
const FICHIER: &str = "connexion.bin";
/// L'« entropie » de DPAPI : un autre programme de la session doit la connaître pour déchiffrer le fichier.
const ENTROPIE: &[u8] = b"Eve/Constructo AI/connexion/1";

/// Où une Eve se relie, et où elle garde ses jetons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lien {
    /// Le serveur MCP de Constructo AI, sans `/mcp` ni barre finale.
    pub serveur: String,
    /// `connexion.bin`, dans le dossier local de cette Eve.
    pub fichier: PathBuf,
}

/// Ce qu'Eve garde de sa connexion.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Etat {
    /// Le serveur qui a émis le reste : un autre serveur (les essais) repart de zéro.
    pub serveur: String,
    pub client_id: String,
    /// L'adresse de retour enregistrée avec ce client : un autre port demande un nouvel enregistrement.
    pub redirection: String,
    pub acces: String,
    /// L'émission et l'échéance du jeton d'accès, en secondes Unix.
    pub emis: u64,
    pub expire: u64,
    pub renouvellement: String,
    /// Le compte de connexion (le courriel de l'entreprise, lu dans le jeton), pour l'écran seulement.
    pub compte: String,
}

/// Les jetons ne s'impriment jamais, pas même dans un message d'essai : leur longueur seulement.
impl std::fmt::Debug for Etat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Etat")
            .field("serveur", &self.serveur)
            .field("client_id", &self.client_id)
            .field("redirection", &self.redirection)
            .field("acces", &format_args!("({} caractères)", self.acces.len()))
            .field("emis", &self.emis)
            .field("expire", &self.expire)
            .field("renouvellement", &format_args!("({} caractères)", self.renouvellement.len()))
            .field("compte", &self.compte)
            .finish()
    }
}

impl Etat {
    /// Eve a de quoi se connecter : un jeton d'accès, ou de quoi en obtenir un.
    pub fn connectee(&self) -> bool {
        !self.acces.is_empty() || !self.renouvellement.is_empty()
    }

    /// Le jeton d'accès sert encore une minute au moins.
    pub fn acces_valable(&self, maintenant: u64) -> bool {
        !self.acces.is_empty() && self.expire > maintenant + 60
    }

    /// Le jeton d'accès arrive à échéance (ou manque) et peut se renouveler. La marge est de dix minutes, ou de la
    /// moitié de sa vie s'il vit moins de vingt minutes : un jeton court ne se renouvelle pas en boucle.
    pub fn a_renouveler(&self, maintenant: u64) -> bool {
        let vie = self.expire.saturating_sub(self.emis);
        let marge = if self.emis == 0 { MARGE } else { MARGE.min(vie / 2) };
        !self.renouvellement.is_empty() && (self.acces.is_empty() || self.expire <= maintenant + marge)
    }

    /// Oublie les jetons (la déconnexion, ou un renouvellement refusé pour de bon) ; l'enregistrement du client reste.
    pub fn sans_jetons(&self) -> Etat {
        Etat {
            acces: String::new(),
            emis: 0,
            expire: 0,
            renouvellement: String::new(),
            compte: String::new(),
            ..self.clone()
        }
    }
}

/// L'heure, en secondes Unix.
pub fn maintenant() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Une connexion commencée dans le navigateur : ce qu'il faut pour reconnaître son retour. Sans `Debug` : elle porte le
/// secret de PKCE.
#[derive(Clone)]
pub struct EnCours {
    /// Le paramètre `state` : le retour doit le rapporter tel quel.
    pub etat_oauth: String,
    /// Le secret de PKCE, qui ne quitte le poste qu'à l'échange du code.
    pub verificateur: String,
    pub depuis: Instant,
}

/// Pourquoi un renouvellement a échoué.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refus {
    /// Le serveur refuse pour de bon (jeton révoqué ou expiré, entreprise désactivée…) : il faut se reconnecter.
    Definitif(String),
    /// Le réseau, ou le serveur en panne (ou qui se réveille) : on réessaiera.
    Passager(String),
}

/// Le retour du navigateur, tel que la vitrine l'a reçu.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Retour {
    pub code: String,
    pub etat_oauth: String,
    pub emetteur: String,
    pub erreur: String,
    pub description: String,
}

impl Lien {
    /// Celui de cette Eve : le serveur de Constructo AI (ou celui que nomme `EVE_CONSTRUCTO_MCP`), et le fichier de son
    /// dossier local.
    pub fn du_poste() -> Lien {
        let serveur = std::env::var("EVE_CONSTRUCTO_MCP")
            .ok()
            .map(|s| s.trim().trim_end_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| SERVEUR_PAR_DEFAUT.to_string());
        Lien { serveur, fichier: dossier_local().join(FICHIER) }
    }

    /// Un autre serveur que celui de Constructo AI (un faux, pour un essai) : le journal le dit.
    pub fn d_essai(&self) -> bool {
        self.serveur != SERVEUR_PAR_DEFAUT
    }

    /// L'adresse du connecteur MCP, qui est aussi la « ressource » OAuth (RFC 8707) : le serveur refuse toute autre
    /// valeur.
    pub fn ressource(&self) -> String {
        format!("{}/mcp", self.serveur)
    }

    /// L'état gardé sur ce poste ; rien (ou illisible, ou d'un autre serveur) : un état vide.
    pub fn charger(&self) -> Etat {
        let Ok(chiffre) = fs::read(&self.fichier) else { return Etat::default() };
        let etat = dechiffrer(&chiffre).ok().and_then(|o| serde_json::from_slice::<Etat>(&o).ok()).unwrap_or_default();
        if etat.serveur == self.serveur { etat } else { Etat::default() }
    }

    /// Écrit l'état, chiffré, d'un seul geste ([`ecrire_d_un_geste`]).
    pub fn enregistrer(&self, etat: &Etat) -> Result<(), String> {
        let clair = serde_json::to_vec(etat).map_err(|e| e.to_string())?;
        let chiffre = chiffrer(&clair)?;
        ecrire_d_un_geste(&self.fichier, &chiffre).map_err(|e| format!("la connexion ne s'écrit pas ({e})"))
    }

    /// La première étape (dans un fil : elle parle au serveur) : l'enregistrement d'Eve si son client manque ou ne vaut
    /// pas pour ce port, puis l'adresse de la page de connexion. Rend l'état (avec son client, déjà écrit) et la
    /// connexion en cours.
    pub fn preparer(&self, etat: &Etat, port: u16) -> Result<(Etat, EnCours, String), String> {
        let redirection = format!("http://127.0.0.1:{port}{CHEMIN_RETOUR}");
        let mut etat = etat.clone();
        if etat.client_id.is_empty() || etat.redirection != redirection || etat.serveur != self.serveur {
            let client_id = self.enregistrer_le_client(&redirection)?;
            etat = Etat { serveur: self.serveur.clone(), client_id, redirection, ..Etat::default() };
            self.enregistrer(&etat)?;
        }
        let en_cours =
            EnCours { etat_oauth: aleatoire_b64(32)?, verificateur: aleatoire_b64(32)?, depuis: Instant::now() };
        let url = self.adresse_d_autorisation(&etat, &en_cours.etat_oauth, &defi(&en_cours.verificateur)?);
        Ok((etat, en_cours, url))
    }

    /// L'enregistrement dynamique (RFC 7591) : un client public, son adresse de retour.
    fn enregistrer_le_client(&self, redirection: &str) -> Result<String, String> {
        let demande = json!({
            "client_name": NOM_CLIENT,
            "redirect_uris": [redirection],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        });
        let (code, corps) =
            https::post_json(&format!("{}/register", self.serveur), &demande, DELAI_HTTP).map_err(injoignable)?;
        let v: Value = serde_json::from_slice(&corps).unwrap_or(Value::Null);
        match (code, v.get("client_id").and_then(Value::as_str)) {
            (200 | 201, Some(id)) if !id.is_empty() && id.len() <= 200 => Ok(id.to_string()),
            (429, _) => Err("Constructo AI refuse pour l'instant de nouveaux enregistrements depuis ce réseau : \
                             réessaie dans une heure."
                .into()),
            _ => Err(format!("Constructo AI n'a pas enregistré Eve ({}).", raison_du_serveur(code, &v))),
        }
    }

    /// L'adresse de la page de connexion.
    pub fn adresse_d_autorisation(&self, etat: &Etat, etat_oauth: &str, defi: &str) -> String {
        let ressource = self.ressource();
        let champs = [
            ("response_type", "code"),
            ("client_id", etat.client_id.as_str()),
            ("redirect_uri", etat.redirection.as_str()),
            ("code_challenge", defi),
            ("code_challenge_method", "S256"),
            ("state", etat_oauth),
            ("scope", PORTEE),
            ("resource", &ressource),
        ]
        .iter()
        .map(|(n, v)| format!("{n}={}", https::encoder(v)))
        .collect::<Vec<_>>()
        .join("&");
        format!("{}/authorize?{champs}", self.serveur)
    }

    /// Le retour vaut-il pour la connexion en cours ? Le `state` doit être le sien, l'émetteur (s'il est dit, RFC 9207)
    /// celui du serveur, et la connexion pas trop vieille. Rend le code, ou ce qui ne va pas.
    pub fn verifier_le_retour(&self, en_cours: Option<&EnCours>, r: &Retour) -> Result<String, String> {
        let Some(c) = en_cours else { return Err("aucune connexion n'était en cours".into()) };
        if r.etat_oauth.is_empty() || !egaux(&r.etat_oauth, &c.etat_oauth) {
            return Err("ce retour ne correspond pas à la connexion en cours".into());
        }
        if c.depuis.elapsed() > DELAI_CONNEXION {
            return Err("la connexion a pris plus de dix minutes : recommence".into());
        }
        if !r.erreur.is_empty() {
            let d = if r.description.is_empty() { r.erreur.as_str() } else { r.description.as_str() };
            return Err(format!("Constructo AI a refusé la connexion : {}", ecourter(d, 300)));
        }
        if !r.emetteur.is_empty() && r.emetteur.trim_end_matches('/') != self.serveur {
            return Err("ce retour ne vient pas de Constructo AI".into());
        }
        if r.code.is_empty() || r.code.len() > 512 {
            return Err("le retour ne porte pas de code".into());
        }
        Ok(r.code.clone())
    }

    /// La seconde étape (dans un fil) : le code contre les jetons. L'état rendu est déjà écrit sur le disque.
    pub fn echanger(&self, etat: &Etat, en_cours: &EnCours, code: &str) -> Result<Etat, String> {
        let ressource = self.ressource();
        let champs = [
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", etat.redirection.as_str()),
            ("code_verifier", en_cours.verificateur.as_str()),
            ("client_id", etat.client_id.as_str()),
            ("resource", &ressource),
        ];
        let (statut, corps) =
            https::post_formulaire(&format!("{}/token", self.serveur), &champs, DELAI_HTTP).map_err(injoignable)?;
        let nouveau = lire_les_jetons(etat, statut, &corps).map_err(|r| match r {
            Refus::Definitif(m) | Refus::Passager(m) => m,
        })?;
        self.enregistrer(&nouveau)?;
        Ok(nouveau)
    }

    /// Le renouvellement (dans un fil). L'état rendu est déjà écrit sur le disque : l'ancien jeton de renouvellement est
    /// mort dès la réponse du serveur.
    pub fn renouveler(&self, etat: &Etat) -> Result<Etat, Refus> {
        let champs = [
            ("grant_type", "refresh_token"),
            ("refresh_token", etat.renouvellement.as_str()),
            ("client_id", etat.client_id.as_str()),
        ];
        let (statut, corps) = https::post_formulaire(&format!("{}/token", self.serveur), &champs, DELAI_HTTP)
            .map_err(|e| Refus::Passager(injoignable(e)))?;
        let nouveau = lire_les_jetons(etat, statut, &corps)?;
        self.enregistrer(&nouveau).map_err(Refus::Passager)?;
        Ok(nouveau)
    }

    /// Le serveur MCP de Constructo AI, pour la demande `mcp_set_servers` (aucun jeton : le serveur est retiré).
    pub fn serveurs_mcp(&self, acces: Option<&str>) -> Value {
        match acces {
            Some(j) => json!({ SERVEUR_CONSTRUCTO: {
                "type": "http",
                "url": self.ressource(),
                "headers": { "Authorization": format!("Bearer {j}") },
            } }),
            None => json!({}),
        }
    }
}

/// Comparaison sans court-circuit (le `state` est un secret de la séance).
pub fn egaux(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// La réponse du point `/token` : les nouveaux jetons, ou le refus du serveur.
pub fn lire_les_jetons(etat: &Etat, statut: u16, corps: &[u8]) -> Result<Etat, Refus> {
    let v: Value = serde_json::from_slice(corps).unwrap_or(Value::Null);
    let champ = |cle: &str| v.get(cle).and_then(Value::as_str).unwrap_or("").to_string();
    if statut == 200 {
        let (acces, renouvellement) = (champ("access_token"), champ("refresh_token"));
        let forme = |j: &str| !j.is_empty() && j.len() <= 8192 && j.bytes().all(|b| b.is_ascii_graphic());
        if !forme(&acces) || !forme(&renouvellement) {
            return Err(Refus::Passager("la réponse de Constructo AI ne porte pas de jetons valables".into()));
        }
        let duree = v.get("expires_in").and_then(Value::as_u64).unwrap_or(3600).clamp(60, 24 * 3600);
        let compte = compte_du_jeton(&acces).unwrap_or_else(|| etat.compte.clone());
        let emis = maintenant();
        return Ok(Etat { acces, emis, expire: emis + duree, renouvellement, compte, ..etat.clone() });
    }
    let raison = raison_du_serveur(statut, &v);
    // `invalid_grant` : le jeton est révoqué, expiré ou déjà servi, ou l'entreprise n'y a plus droit — le serveur ne
    // changera pas d'avis. Le reste (429, 5xx, une page d'erreur de Render qui se réveille) est passager.
    if (400..500).contains(&statut) && statut != 429 && statut != 408 {
        Err(Refus::Definitif(raison))
    } else {
        Err(Refus::Passager(raison))
    }
}

/// La raison d'un refus, telle que le serveur la donne (ses messages sont en français : l'abonnement EG seulement,
/// l'entreprise désactivée…), écourtée ; sinon le code HTTP.
fn raison_du_serveur(statut: u16, v: &Value) -> String {
    let d = v.get("error_description").or_else(|| v.get("error")).and_then(Value::as_str).unwrap_or("").trim();
    if d.is_empty() { format!("HTTP {statut}") } else { ecourter(d, 300) }
}

fn injoignable(e: String) -> String {
    format!("Constructo AI ne répond pas ({e})")
}

/// Le courriel de connexion, lu dans le jeton d'accès (un JWT : `sub`). Pour l'écran seulement : rien n'en dépend.
pub fn compte_du_jeton(jeton: &str) -> Option<String> {
    let charge = jeton.split('.').nth(1)?;
    let v: Value = serde_json::from_slice(&b64url_decoder(charge)?).ok()?;
    let sub = v.get("sub")?.as_str()?.trim();
    let propre: String = sub.chars().filter(|c| !c.is_control()).take(120).collect();
    (!propre.is_empty()).then_some(propre)
}

/// La réponse à `mcp_set_servers` : l'erreur du serveur de Constructo AI, s'il en a une (`errors.constructo-ai`).
pub fn erreur_du_branchement(corps: &Value) -> Option<String> {
    let e = corps.get("errors")?.get(SERVEUR_CONSTRUCTO)?;
    let t = e.as_str().map(str::to_string).unwrap_or_else(|| e.to_string());
    // Le message de Claude Code peut citer la configuration : on n'en garde que le début, sans jeton possible.
    Some(ecourter(&sans_porteur(&t), 200))
}

/// Un texte sans jeton : tout ce qui suit « Bearer » s'efface, et tout mot qui a la forme d'un JWT (`eyJ…`). Un message
/// d'erreur de Claude Code peut citer la configuration du serveur, son en-tête compris : il ne va au journal qu'ainsi.
pub fn sans_porteur(t: &str) -> String {
    let coupe = match t.to_ascii_lowercase().find("bearer") {
        Some(i) => format!("{}Bearer …", &t[..i]),
        None => t.to_string(),
    };
    coupe
        .split(' ')
        .map(|m| if m.trim_start_matches(['"', '\'']).starts_with("eyJ") && m.len() > 20 { "…" } else { m })
        .collect::<Vec<_>>()
        .join(" ")
}

// ------------------------------------------------------------------ PKCE, aléa, base64

/// Le défi PKCE (S256) : le SHA-256 du vérificateur, en base64url sans remplissage (RFC 7636).
pub fn defi(verificateur: &str) -> Result<String, String> {
    Ok(b64url(&sha256(verificateur.as_bytes())?))
}

/// `n` octets du générateur du système, en base64url (un `state`, un vérificateur de 43 caractères pour 32 octets).
pub fn aleatoire_b64(n: usize) -> Result<String, String> {
    let mut o = vec![0u8; n];
    aleatoire(&mut o)?;
    Ok(b64url(&o))
}

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Le base64url sans remplissage.
pub fn b64url(octets: &[u8]) -> String {
    let mut s = String::with_capacity(octets.len().div_ceil(3) * 4);
    for bloc in octets.chunks(3) {
        let n = (u32::from(bloc[0]) << 16)
            | (u32::from(*bloc.get(1).unwrap_or(&0)) << 8)
            | u32::from(*bloc.get(2).unwrap_or(&0));
        for (i, decalage) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= bloc.len() {
                s.push(char::from(B64URL[((n >> decalage) & 63) as usize]));
            }
        }
    }
    s
}

/// Le base64url (avec ou sans remplissage) ; `None` sur un caractère étranger.
pub fn b64url_decoder(s: &str) -> Option<Vec<u8>> {
    let s = s.trim_end_matches('=');
    let mut sortie = Vec::with_capacity(s.len() * 3 / 4);
    let (mut tampon, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = B64URL.iter().position(|&x| x == c).or(match c {
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        })? as u32;
        tampon = (tampon << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            sortie.push((tampon >> bits) as u8);
            tampon &= (1 << bits) - 1;
        }
    }
    Some(sortie)
}

/// Ouvre une adresse dans le navigateur par défaut du poste (ShellExecute : ni cmd.exe ni guillemets à échapper, alors
/// que l'adresse est pleine de `&`).
pub fn ouvrir_dans_le_navigateur(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://127.0.0.1:")) || url.chars().any(char::is_control) {
        return Err("adresse refusée".into());
    }
    // `EVE_SANS_NAVIGATEUR` : les essais de bout en bout, sans navigateur. La page du faux serveur (sur le poste) se
    // joue comme le ferait le navigateur, redirection vers la vitrine comprise ; une autre ne s'ouvre pas.
    if std::env::var_os("EVE_SANS_NAVIGATEUR").is_some() {
        if url.starts_with("http://127.0.0.1:") {
            return crate::essais::jouer_le_navigateur(url);
        }
        crate::journal::ecrire("navigateur (essai) : la page n'est pas ouverte (EVE_SANS_NAVIGATEUR)");
        return Ok(());
    }
    imp::ouvrir(url)
}

fn sha256(o: &[u8]) -> Result<[u8; 32], String> {
    imp::sha256(o)
}

fn aleatoire(o: &mut [u8]) -> Result<(), String> {
    imp::aleatoire(o)
}

fn chiffrer(o: &[u8]) -> Result<Vec<u8>, String> {
    imp::proteger(o, true)
}

fn dechiffrer(o: &[u8]) -> Result<Vec<u8>, String> {
    imp::proteger(o, false)
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        BCRYPT_SHA256_ALG_HANDLE, BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom, BCryptHash, CRYPT_INTEGER_BLOB,
        CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{PCWSTR, w};

    use super::ENTROPIE;

    pub fn sha256(o: &[u8]) -> Result<[u8; 32], String> {
        let mut h = [0u8; 32];
        // SAFETY : la pseudo-poignée de SHA-256 (Windows 10 et plus) n'a pas à s'ouvrir ; l'entrée et la sortie sont
        // des tranches valides pendant l'appel, la sortie fait exactement la taille d'un SHA-256.
        let s = unsafe { BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, o, &mut h) };
        if s.is_ok() { Ok(h) } else { Err(format!("le hachage a échoué ({:#x})", s.0)) }
    }

    pub fn aleatoire(o: &mut [u8]) -> Result<(), String> {
        // SAFETY : aucune poignée (le générateur préféré du système) ; la tranche est valide pendant l'appel.
        let s = unsafe { BCryptGenRandom(None, o, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
        if s.is_ok() { Ok(()) } else { Err(format!("le générateur du système a échoué ({:#x})", s.0)) }
    }

    /// DPAPI, pour la session de l'utilisateur : `chiffrer` (vrai) ou déchiffrer.
    pub fn proteger(o: &[u8], chiffrer: bool) -> Result<Vec<u8>, String> {
        let n = u32::try_from(o.len()).map_err(|_| "trop gros".to_string())?;
        let entree = CRYPT_INTEGER_BLOB { cbData: n, pbData: o.as_ptr().cast_mut() };
        let entropie = CRYPT_INTEGER_BLOB { cbData: ENTROPIE.len() as u32, pbData: ENTROPIE.as_ptr().cast_mut() };
        let mut sortie = CRYPT_INTEGER_BLOB::default();
        // SAFETY : les blocs d'entrée et d'entropie pointent sur des tranches valides pendant l'appel, que DPAPI ne fait
        // que lire ; la sortie est allouée par DPAPI (LocalAlloc), copiée puis libérée ci-dessous ; aucune interface.
        let r = unsafe {
            if chiffrer {
                CryptProtectData(
                    &entree,
                    w!("Eve - connexion Constructo AI"),
                    Some(&entropie),
                    None,
                    None,
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut sortie,
                )
            } else {
                CryptUnprotectData(&entree, None, Some(&entropie), None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut sortie)
            }
        };
        r.map_err(|e| format!("DPAPI : {e}"))?;
        // SAFETY : DPAPI a rempli `sortie` d'un bloc de `cbData` octets, valide jusqu'à sa libération.
        let octets = unsafe { std::slice::from_raw_parts(sortie.pbData, sortie.cbData as usize) }.to_vec();
        // SAFETY : le bloc vient de DPAPI (LocalAlloc) et ne sert plus.
        unsafe {
            let _ = LocalFree(Some(HLOCAL(sortie.pbData.cast())));
        }
        Ok(octets)
    }

    pub fn ouvrir(url: &str) -> Result<(), String> {
        let large: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY : l'adresse est une chaîne terminée par un zéro qui vit pendant l'appel ; ni fenêtre parente, ni
        // paramètres, ni dossier.
        let h = unsafe {
            ShellExecuteW(None, w!("open"), PCWSTR(large.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL)
        };
        // Au-delà de 32 : réussi (la convention de ShellExecute).
        if h.0 as usize > 32 { Ok(()) } else { Err(format!("le navigateur ne s'ouvre pas (code {})", h.0 as usize)) }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn sha256(_o: &[u8]) -> Result<[u8; 32], String> {
        Err("Windows seulement".into())
    }
    pub fn aleatoire(_o: &mut [u8]) -> Result<(), String> {
        Err("Windows seulement".into())
    }
    pub fn proteger(_o: &[u8], _chiffrer: bool) -> Result<Vec<u8>, String> {
        Err("Windows seulement".into())
    }
    pub fn ouvrir(_url: &str) -> Result<(), String> {
        Err("Windows seulement".into())
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    /// Un lien vers `serveur`, son fichier dans un dossier temporaire à lui (`nom` : un par essai, qui tournent ensemble).
    fn lien_d_essai(nom: &str, serveur: &str) -> Lien {
        let dossier = std::env::temp_dir().join(format!("eve-connexion-{}-{nom}", std::process::id()));
        fs::create_dir_all(&dossier).unwrap();
        Lien { serveur: serveur.to_string(), fichier: dossier.join(FICHIER) }
    }

    /// L'exemple de la RFC 7636 (annexe B) : le défi S256 d'un vérificateur connu.
    #[test]
    fn le_defi_de_la_rfc_7636() {
        assert_eq!(
            defi("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").unwrap(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn le_base64url_fait_l_aller_retour() {
        for n in 0..70usize {
            let o: Vec<u8> = (0..n).map(|i| (i * 37 + 11) as u8).collect();
            let s = b64url(&o);
            assert!(!s.contains(['=', '+', '/']), "{s}");
            assert_eq!(b64url_decoder(&s).unwrap(), o, "{n}");
        }
        assert_eq!(b64url(b"Man"), "TWFu");
        assert_eq!(b64url(b"Ma"), "TWE");
        assert!(b64url_decoder("a b").is_none());
    }

    #[test]
    fn l_alea_change_et_a_la_bonne_longueur() {
        let a = aleatoire_b64(32).unwrap();
        let b = aleatoire_b64(32).unwrap();
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
    }

    #[test]
    fn le_compte_se_lit_dans_le_jeton() {
        let charge = b64url(br#"{"iss":"x","sub":"info@exemple.ca","tenant_schema":"tenant_x"}"#);
        assert_eq!(compte_du_jeton(&format!("e30.{charge}.signature")).as_deref(), Some("info@exemple.ca"));
        assert_eq!(compte_du_jeton("pas-un-jwt"), None);
        assert_eq!(compte_du_jeton("a.!!!.c"), None);
    }

    #[test]
    fn le_serveur_par_defaut_est_celui_de_constructo_ai() {
        let l = Lien { serveur: SERVEUR_PAR_DEFAUT.into(), fichier: PathBuf::from("x") };
        assert!(!l.d_essai());
        assert_eq!(l.ressource(), "https://constructo-mcp.onrender.com/mcp");
        assert!(Lien { serveur: "http://127.0.0.1:47997".into(), ..l }.d_essai());
    }

    #[test]
    fn l_adresse_d_autorisation_porte_tout_ce_qu_exige_le_serveur() {
        let l = Lien { serveur: "http://127.0.0.1:47997".into(), fichier: PathBuf::from("x") };
        let etat = Etat {
            client_id: "mcp_abc".into(),
            redirection: "http://127.0.0.1:47821/connexion/retour".into(),
            ..Etat::default()
        };
        let url = l.adresse_d_autorisation(&etat, "etat-1", "defi-1");
        assert!(url.starts_with("http://127.0.0.1:47997/authorize?"), "{url}");
        for attendu in [
            "response_type=code",
            "client_id=mcp_abc",
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A47821%2Fconnexion%2Fretour",
            "code_challenge=defi-1",
            "code_challenge_method=S256",
            "state=etat-1",
            "scope=mcp",
            "resource=http%3A%2F%2F127.0.0.1%3A47997%2Fmcp",
        ] {
            assert!(url.contains(attendu), "{attendu} manque dans {url}");
        }
    }

    fn en_cours() -> EnCours {
        EnCours { etat_oauth: "etat-secret".into(), verificateur: "v".into(), depuis: Instant::now() }
    }

    #[test]
    fn un_retour_se_verifie() {
        let l = Lien { serveur: "http://127.0.0.1:47997".into(), fichier: PathBuf::from("x") };
        let c = en_cours();
        let bon = Retour {
            code: "code-1".into(),
            etat_oauth: "etat-secret".into(),
            emetteur: l.serveur.clone(),
            ..Retour::default()
        };
        assert_eq!(l.verifier_le_retour(Some(&c), &bon).unwrap(), "code-1");
        // Sans connexion en cours, avec un autre `state`, un autre émetteur, ou sans code : refusé.
        assert!(l.verifier_le_retour(None, &bon).is_err());
        assert!(l.verifier_le_retour(Some(&c), &Retour { etat_oauth: "etat-autre".into(), ..bon.clone() }).is_err());
        assert!(l.verifier_le_retour(Some(&c), &Retour { etat_oauth: String::new(), ..bon.clone() }).is_err());
        let ailleurs = Retour { emetteur: "https://ailleurs.exemple".into(), ..bon.clone() };
        assert!(l.verifier_le_retour(Some(&c), &ailleurs).is_err());
        assert!(l.verifier_le_retour(Some(&c), &Retour { code: String::new(), ..bon.clone() }).is_err());
        // L'émetteur peut manquer (un serveur d'avant la RFC 9207).
        assert!(l.verifier_le_retour(Some(&c), &Retour { emetteur: String::new(), ..bon.clone() }).is_ok());
        // Un refus du serveur se dit avec sa raison.
        let refus = Retour { erreur: "access_denied".into(), description: "Accès révoqué".into(), ..bon.clone() };
        assert!(l.verifier_le_retour(Some(&c), &refus).unwrap_err().contains("Accès révoqué"));
        // Trop vieux.
        let vieux = EnCours { depuis: Instant::now() - DELAI_CONNEXION - Duration::from_secs(1), ..c };
        assert!(l.verifier_le_retour(Some(&vieux), &bon).is_err());
    }

    #[test]
    fn les_jetons_se_lisent() {
        let etat = Etat { client_id: "c".into(), compte: "ancien@x.ca".into(), ..Etat::default() };
        let ok =
            br#"{"access_token":"a.b.c","token_type":"Bearer","expires_in":3600,"refresh_token":"r-1","scope":"mcp"}"#;
        let e = lire_les_jetons(&etat, 200, ok).unwrap();
        assert_eq!((e.acces.as_str(), e.renouvellement.as_str(), e.client_id.as_str()), ("a.b.c", "r-1", "c"));
        assert!(e.expire >= maintenant() + 3590 && e.expire <= maintenant() + 3600);
        // Un jeton illisible garde l'ancien compte affiché.
        assert_eq!(e.compte, "ancien@x.ca");
        assert!(e.acces_valable(maintenant()) && !e.a_renouveler(maintenant()));
        // Un 200 sans jetons, ou avec un jeton qui glisserait un en-tête : passager, rien n'est gardé.
        assert!(matches!(lire_les_jetons(&etat, 200, br#"{"access_token":""}"#), Err(Refus::Passager(_))));
        assert!(matches!(
            lire_les_jetons(&etat, 200, b"{\"access_token\":\"a\\r\\nX: y\",\"refresh_token\":\"r\"}"),
            Err(Refus::Passager(_))
        ));
        // Le refus du serveur garde sa raison, en français.
        let refus = r#"{"error":"invalid_grant","error_description":"Refresh token deja utilisé"}"#.as_bytes();
        assert_eq!(lire_les_jetons(&etat, 400, refus), Err(Refus::Definitif("Refresh token deja utilisé".into())));
        assert!(matches!(lire_les_jetons(&etat, 429, b"{}"), Err(Refus::Passager(_))));
        // Render qui se réveille rend une page d'erreur : passager.
        assert!(matches!(lire_les_jetons(&etat, 502, b"<html>"), Err(Refus::Passager(_))));
    }

    #[test]
    fn l_echeance_decide_du_renouvellement() {
        let t = 1_000_000;
        let e = Etat { acces: "a".into(), renouvellement: "r".into(), expire: t + 3600, ..Etat::default() };
        assert!(e.acces_valable(t) && !e.a_renouveler(t));
        let bientot = Etat { expire: t + MARGE - 1, ..e.clone() };
        assert!(bientot.acces_valable(t) && bientot.a_renouveler(t));
        let echu = Etat { expire: t - 1, ..e.clone() };
        assert!(!echu.acces_valable(t) && echu.a_renouveler(t));
        let sans = e.sans_jetons();
        assert!(!sans.connectee() && !sans.a_renouveler(t));
        // Un jeton de deux minutes ne se renouvelle pas en boucle : à la moitié de sa vie, pas dès sa naissance.
        let court = Etat { emis: t, expire: t + 120, ..e.clone() };
        assert!(!court.a_renouveler(t) && !court.a_renouveler(t + 59) && court.a_renouveler(t + 60));
        let d_une_heure = Etat { emis: t, expire: t + 3600, ..e.clone() };
        assert!(!d_une_heure.a_renouveler(t + 2999) && d_une_heure.a_renouveler(t + 3000));
    }

    #[test]
    fn le_branchement_porte_le_jeton_et_l_erreur_ne_le_repete_jamais() {
        let l = Lien { serveur: "http://127.0.0.1:47997".into(), fichier: PathBuf::from("x") };
        let s = l.serveurs_mcp(Some("jeton-1"));
        assert_eq!(s[SERVEUR_CONSTRUCTO]["type"], "http");
        assert_eq!(s[SERVEUR_CONSTRUCTO]["url"], "http://127.0.0.1:47997/mcp");
        assert_eq!(s[SERVEUR_CONSTRUCTO]["headers"]["Authorization"], "Bearer jeton-1");
        assert_eq!(l.serveurs_mcp(None), json!({}));
        let r = json!({ "added": [SERVEUR_CONSTRUCTO],
            "errors": { SERVEUR_CONSTRUCTO: "rejected header Bearer jeton-secret (HTTP 401)" } });
        let e = erreur_du_branchement(&r).unwrap();
        assert!(!e.contains("jeton-secret"), "{e}");
        assert!(erreur_du_branchement(&json!({ "added": [SERVEUR_CONSTRUCTO], "errors": {} })).is_none());
        // Un JWT cité sans « Bearer » s'efface aussi.
        let jwt = format!("eyJ{}.eyJzdWIiOiJ4In0.sig", "a".repeat(30));
        assert!(!sans_porteur(&format!("config {{\"Authorization\": \"{jwt}\"}}")).contains(&jwt));
        assert_eq!(sans_porteur("HTTP 401 Unauthorized"), "HTTP 401 Unauthorized");
    }

    #[test]
    fn un_etat_ne_montre_jamais_ses_jetons() {
        let e =
            Etat { acces: "secret-acces".into(), renouvellement: "secret-renouvellement".into(), ..Etat::default() };
        let vu = format!("{e:?}");
        assert!(!vu.contains("secret") && vu.contains("12 caractères"), "{vu}");
    }

    #[test]
    fn dpapi_fait_l_aller_retour() {
        let clair = "{\"acces\":\"secret\"}".as_bytes();
        let chiffre = chiffrer(clair).unwrap();
        assert!(!chiffre.windows(6).any(|w| w == b"secret"));
        assert_eq!(dechiffrer(&chiffre).unwrap(), clair);
        assert!(dechiffrer(b"pas chiffre").is_err());
    }

    /// Un état d'un autre serveur (les essais) ne vaut pas pour celui-ci, et un fichier illisible non plus.
    #[test]
    fn un_etat_ne_vaut_que_pour_son_serveur() {
        let l = lien_d_essai("serveur", "http://127.0.0.1:1");
        let e = Etat { serveur: l.serveur.clone(), client_id: "c".into(), acces: "acces-1".into(), ..Etat::default() };
        l.enregistrer(&e).unwrap();
        assert_eq!(l.charger(), e);
        assert!(!fs::read(&l.fichier).unwrap().windows(7).any(|w| w == b"acces-1"), "chiffré sur le disque");
        let autre = Lien { serveur: "http://127.0.0.1:2".into(), ..l.clone() };
        assert_eq!(autre.charger(), Etat::default());
        fs::write(&l.fichier, b"illisible").unwrap();
        assert_eq!(l.charger(), Etat::default());
        let _ = fs::remove_file(&l.fichier);
    }

    /// La connexion de bout en bout, sur le faux Constructo AI d'`eve --essai-constructo`, monté ici sur un port libre :
    /// l'enregistrement, la page de connexion (jouée comme le ferait le navigateur), l'échange du code contre les
    /// jetons, gardés chiffrés ; puis le renouvellement, qui fait tourner le jeton — et l'ancien, resservi, révoque toute
    /// la famille, comme le vrai serveur.
    #[test]
    fn la_connexion_de_bout_en_bout_sur_un_faux_serveur() {
        let faux = crate::essais::FauxConstructo::demarrer(0, 3600, 0).unwrap();
        let l = lien_d_essai("bout-en-bout", &format!("http://127.0.0.1:{}", faux.port));
        let _ = fs::remove_file(&l.fichier);
        // 1. L'enregistrement d'Eve, gardé avant même la connexion ; puis la page de connexion.
        let (etat, en_cours, url) = l.preparer(&Etat::default(), 47821).unwrap();
        assert!(etat.client_id.starts_with("mcp_"), "{etat:?}");
        assert_eq!(etat.redirection, "http://127.0.0.1:47821/connexion/retour");
        assert_eq!(l.charger(), etat);
        // 2. Le navigateur : le faux serveur accorde aussitôt et renvoie vers la vitrine, avec le code et le `state`.
        let (statut, ou) = crate::essais::demander_sans_suivre(&url).unwrap();
        assert_eq!(statut, 302);
        let ou = ou.unwrap();
        assert!(ou.starts_with("http://127.0.0.1:47821/connexion/retour?"), "{ou}");
        let (_, requete) = ou.split_once('?').unwrap();
        let param = |nom: &str| {
            let valeur = requete.split('&').filter_map(|p| p.split_once('=')).find(|(n, _)| *n == nom).map(|(_, v)| v);
            crate::ecran::decoder(valeur.unwrap_or_default())
        };
        let retour =
            Retour { code: param("code"), etat_oauth: param("state"), emetteur: param("iss"), ..Retour::default() };
        let code = l.verifier_le_retour(Some(&en_cours), &retour).unwrap();
        // 3. Le code contre les jetons ; un mauvais vérificateur de PKCE, ou le même code une seconde fois : refusés.
        let faux_secret =
            EnCours { verificateur: "un-autre-secret-de-quarante-trois-caracteres".into(), ..en_cours.clone() };
        assert!(l.echanger(&etat, &faux_secret, &code).is_err(), "PKCE vérifié par le serveur");
        let e1 = l.echanger(&etat, &en_cours, &code).unwrap();
        assert!(e1.connectee() && e1.acces_valable(maintenant()), "{e1:?}");
        assert_eq!(e1.compte, "info@exemple.ca");
        assert_eq!(l.charger(), e1, "les jetons sont gardés, chiffrés");
        assert!(
            !fs::read(&l.fichier).unwrap().windows(e1.renouvellement.len()).any(|w| w == e1.renouvellement.as_bytes())
        );
        assert!(l.echanger(&etat, &en_cours, &code).is_err(), "un code ne sert qu'une fois");
        // 4. Le renouvellement : de nouveaux jetons, écrits avant de servir.
        let e2 = l.renouveler(&e1).unwrap();
        assert_ne!(e2.renouvellement, e1.renouvellement);
        assert_ne!(e2.acces, e1.acces);
        assert_eq!(l.charger(), e2);
        // 5. L'ancien jeton de renouvellement, resservi : refusé pour de bon, et la famille révoquée.
        assert!(matches!(l.renouveler(&e1), Err(Refus::Definitif(_))));
        assert!(matches!(l.renouveler(&e2), Err(Refus::Definitif(_))), "toute la famille est révoquée");
        // Le faux serveur a tout vu passer, sans jamais un jeton dans son journal.
        let journal = faux.journal().join("\n");
        for etape in
            ["register", "authorize accordé", "token/code accordé", "token/renouvellement accordé", "RÉUTILISÉ"]
        {
            assert!(journal.contains(etape), "{etape} manque :\n{journal}");
        }
        assert!(!journal.contains(&e2.acces) && !journal.contains(&e1.renouvellement), "{journal}");
        let _ = fs::remove_file(&l.fichier);
    }

    /// Un serveur qui ne répond pas : la panne se dit, passagère.
    #[test]
    fn un_serveur_absent_est_une_panne_passagere() {
        let l = lien_d_essai("absent", "http://127.0.0.1:9");
        let e =
            Etat { serveur: l.serveur.clone(), client_id: "c".into(), renouvellement: "r".into(), ..Etat::default() };
        match l.renouveler(&e) {
            Err(Refus::Passager(m)) => assert!(m.starts_with("Constructo AI ne répond pas"), "{m}"),
            autre => panic!("{autre:?}"),
        }
        // (`EnCours` n'a pas de `Debug` : il porte le secret de PKCE.)
        let erreur = l.preparer(&Etat::default(), 47821).err().unwrap_or_default();
        assert!(erreur.starts_with("Constructo AI ne répond pas"), "{erreur}");
    }
}
