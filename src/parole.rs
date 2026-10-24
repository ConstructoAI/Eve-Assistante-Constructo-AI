//! La parole : ce qui se dit à voix haute, et ce que la personne a voulu dire.
//!
//! L'Adjointe écrit pour l'écran ; Eve n'en lit que l'essentiel. La convention, donnée à l'Adjointe par la consigne
//! du mode vocal : ce qui précède une ligne de trois tirets (`---`) se dit, le reste s'affiche seulement. Le markdown
//! se retire (une voix lirait « astérisque »), les tableaux et les blocs de code ne se lisent pas, et une réponse trop
//! longue s'arrête à une fin de phrase : « Le reste est à l'écran. »
//!
//! Dans l'autre sens, la transcription de Whisper se nettoie (ses hallucinations sur un silence, les annotations entre
//! crochets), et deux réponses se reconnaissent : l'accord (« je confirme ») et l'arrêt (« arrête »). Un simple
//! « oui » ne vaut pas accord : mal entendu, il ferait partir une écriture.

/// La ligne qui sépare ce qui se dit de ce qui ne s'affiche qu'à l'écran.
pub const SEPARATEUR: &str = "---";

/// Au-delà, la voix s'arrête à une fin de phrase et renvoie à l'écran.
pub const LONGUEUR_MAX: usize = 650;

/// Ce que Whisper invente sur un silence ou un bruit (sous-titres de vidéos de son entraînement).
const HALLUCINATIONS: [&str; 8] = [
    "sous titres realises", "sous titrage", "amara org", "merci d avoir regarde", "abonnez vous",
    "merci de votre attention", "sous titres par", "transcription par",
];

/// Ce qui se dit autrement qu'il s'écrit.
const REMPLACEMENTS: [(&str, &str); 6] =
    [(" → ", ", "), ("→", ", "), (" -> ", ", "), (" => ", ", "), (" & ", " et "), ("≈", "environ ")];

/// Le texte à lire à voix haute.
pub fn pour_la_voix(texte: &str) -> String {
    let mut morceaux: Vec<String> = Vec::new();
    let mut dans_code = false;
    for ligne in texte.lines() {
        let l = ligne.trim();
        if l.starts_with("```") {
            dans_code = !dans_code;
            continue;
        }
        if dans_code || l.is_empty() || l.starts_with('|') {
            continue;
        }
        if est_un_separateur(l) {
            if morceaux.is_empty() {
                continue;
            }
            break;
        }
        let l = propre(sans_marque_de_liste(l));
        if !l.is_empty() {
            morceaux.push(ponctuer(&l));
        }
    }
    couper(&morceaux.join(" "), LONGUEUR_MAX)
}

/// Une ligne de séparation : les trois tirets de la convention (ou plus), ou leurs équivalents en markdown.
fn est_un_separateur(l: &str) -> bool {
    (l.len() >= 3 && l.chars().all(|c| c == '-')) || l == "***" || l == "___"
}

/// Un morceau de texte prêt à dire : sans markdown, sans adresse web, ses flèches et ses symboles en mots.
fn propre(l: &str) -> String {
    let mut s = sans_markdown(l);
    for (de, vers) in REMPLACEMENTS {
        s = s.replace(de, vers);
    }
    s.split_whitespace()
        .filter(|m| !m.starts_with("http://") && !m.starts_with("https://"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// La parole en flux : la réponse de l'Adjointe arrive mot à mot pendant qu'elle l'écrit ; les phrases en sortent dès
/// qu'elles sont complètes, pour que la voix commence avant la fin de la réponse. Les règles sont celles de
/// [`pour_la_voix`] : seul ce qui précède les trois tirets se dit, sans markdown, ni tableau, ni bloc de code (ni
/// carte), ni adresse web ; au-delà de [`LONGUEUR_MAX`] caractères dits dans le tour, « Le reste est à l'écran. »
#[derive(Debug, Default)]
pub struct EnFlux {
    /// Le texte reçu et pas encore dit : la ligne en cours, depuis sa dernière phrase complète.
    tampon: String,
    /// Le début de la ligne en cours a été examiné (liste, titre, tableau, code, tirets).
    ligne_vue: bool,
    /// La ligne en cours ne se dit pas (tableau, code, carte).
    ligne_muette: bool,
    dans_code: bool,
    /// Les trois tirets sont passés : le reste du message ne se dit pas.
    fini: bool,
    /// Quelque chose de ce message a été dit.
    dit_message: bool,
    /// Les caractères dits dans le tour.
    dit: usize,
    /// Le tour a dépassé sa longueur : plus rien ne se dit.
    trop: bool,
}

impl EnFlux {
    /// Un nouveau message de l'Adjointe dans le même tour : les tirets du précédent ne valent plus ; le compte reste.
    pub fn nouveau_message(&mut self) {
        self.tampon.clear();
        self.ligne_vue = false;
        self.ligne_muette = false;
        self.dans_code = false;
        self.fini = false;
        self.dit_message = false;
    }

    /// Un nouveau tour : tout repart de zéro.
    pub fn nouveau_tour(&mut self) {
        *self = EnFlux::default();
    }

    /// Quelque chose a-t-il été dit dans ce tour ?
    pub fn a_parle(&self) -> bool {
        self.dit > 0
    }

    /// Du texte arrive ; rend les phrases prêtes à dire.
    pub fn ajouter(&mut self, morceau: &str) -> Vec<String> {
        self.tampon.push_str(morceau);
        let mut phrases = Vec::new();
        self.extraire(false, &mut phrases);
        phrases
    }

    /// La fin d'un bloc de texte : ce qui reste se dit (une dernière phrase sans point final).
    pub fn finir(&mut self) -> Vec<String> {
        let mut phrases = Vec::new();
        self.extraire(true, &mut phrases);
        self.tampon.clear();
        self.ligne_vue = false;
        self.ligne_muette = false;
        phrases
    }

    fn extraire(&mut self, fin: bool, phrases: &mut Vec<String>) {
        loop {
            if self.fini || self.trop {
                self.tampon.clear();
                return;
            }
            let nl = self.tampon.find('\n');
            if !self.ligne_vue {
                let ligne = match nl {
                    Some(p) => &self.tampon[..p],
                    None => self.tampon.as_str(),
                };
                let l = ligne.trim();
                if l.is_empty() {
                    // Une ligne vide (ou pas encore commencée).
                    match nl {
                        Some(p) => {
                            self.tampon.drain(..=p);
                            continue;
                        }
                        None => {
                            if fin {
                                self.tampon.clear();
                            }
                            return;
                        }
                    }
                }
                if nl.is_none() && !fin {
                    // Trop tôt pour dire ce qu'est la ligne : « - » peut ouvrir une liste ou les trois tirets, « ` »
                    // un bloc de code.
                    if l.chars().count() < 3 || l.chars().all(|c| matches!(c, '-' | '*' | '_' | '`')) {
                        return;
                    }
                }
                if l.starts_with("```") {
                    self.dans_code = !self.dans_code;
                    self.ligne_muette = true;
                } else if self.dans_code || l.starts_with('|') {
                    self.ligne_muette = true;
                } else if est_un_separateur(l) {
                    if self.dit_message {
                        self.fini = true;
                        continue;
                    }
                    // En tête, avant toute parole : on passe outre, comme pour_la_voix.
                    self.ligne_muette = true;
                } else {
                    // Une ligne de texte : sa marque de liste ou de titre se retire (sur la ligne seule : jamais au-delà
                    // de sa fin).
                    let retire = ligne.len() - sans_marque_de_liste(ligne.trim_start_matches([' ', '\t'])).len();
                    self.tampon.drain(..retire);
                }
                self.ligne_vue = true;
            }
            if self.ligne_muette {
                match self.tampon.find('\n') {
                    Some(p) => {
                        self.tampon.drain(..=p);
                        self.ligne_vue = false;
                        self.ligne_muette = false;
                        continue;
                    }
                    None => {
                        if fin {
                            self.tampon.clear();
                        }
                        return;
                    }
                }
            }
            let nl = self.tampon.find('\n');
            let fin_ligne = nl.unwrap_or(self.tampon.len());
            if let Some(p) = fin_de_phrase(&self.tampon[..fin_ligne], nl.is_some() || fin) {
                let phrase: String = self.tampon.drain(..p).collect();
                self.dire(&phrase, false, phrases);
                continue;
            }
            match nl {
                Some(p) => {
                    let phrase: String = self.tampon.drain(..=p).collect();
                    self.ligne_vue = false;
                    self.dire(&phrase, true, phrases);
                }
                None if fin => {
                    let phrase = std::mem::take(&mut self.tampon);
                    self.dire(&phrase, true, phrases);
                    return;
                }
                None => return,
            }
        }
    }

    fn dire(&mut self, brut: &str, fin_de_ligne: bool, phrases: &mut Vec<String>) {
        let t = propre(brut);
        let t = t.trim();
        if t.is_empty() {
            return;
        }
        let t = if fin_de_ligne { ponctuer(t) } else { t.to_string() };
        let n = t.chars().count();
        if self.dit > 0 && self.dit + n > LONGUEUR_MAX {
            phrases.push("Le reste est à l'écran.".into());
            self.trop = true;
            return;
        }
        self.dit += n;
        self.dit_message = true;
        phrases.push(t);
    }
}

/// La fin de la première phrase complète de `s` (l'index qui suit sa ponctuation et ses guillemets fermants), si l'on
/// peut déjà en décider : il faut voir ce qui suit un point, qui peut aussi finir « J.L. » ou une initiale.
/// `ligne_finie` : rien ne viendra plus après `s` sur cette ligne.
fn fin_de_phrase(s: &str, ligne_finie: bool) -> Option<usize> {
    let c: Vec<(usize, char)> = s.char_indices().collect();
    for k in 0..c.len() {
        let ch = c[k].1;
        if !matches!(ch, '.' | '!' | '?' | '…') {
            continue;
        }
        let mut j = k + 1;
        while j < c.len() && matches!(c[j].1, '»' | '"' | ')' | '\'' | '’') {
            j += 1;
        }
        // La ponctuation au bout de ce qu'on a : la suite n'est pas encore là (en fin de ligne, la ligne entière se
        // dit d'un coup).
        if j >= c.len() {
            return None;
        }
        if !c[j].1.is_whitespace() {
            continue;
        }
        let fin = c[j].0;
        if ch == '.' {
            let debut_mot = c[..k].iter().rposition(|(_, x)| x.is_whitespace()).map(|p| p + 1).unwrap_or(0);
            let mot: String = c[debut_mot..k].iter().map(|(_, x)| *x).collect();
            if est_abreviation(&mot) {
                continue;
            }
            let mut m = j;
            while m < c.len() && c[m].1.is_whitespace() {
                m += 1;
            }
            if m >= c.len() {
                if ligne_finie {
                    return Some(fin);
                }
                return None;
            }
            let suivant = c[m].1;
            if !(suivant.is_uppercase()
                || suivant.is_ascii_digit()
                || matches!(suivant, '«' | '"' | '(' | '-' | '—' | '['))
            {
                continue;
            }
        }
        return Some(fin);
    }
    None
}

/// Le mot qui précède un point en fait-il une abréviation plutôt qu'une fin de phrase ?
fn est_abreviation(mot: &str) -> bool {
    const ABREVIATIONS: [&str; 12] = ["mme", "mlle", "st", "ste", "av", "boul", "no", "env", "ex", "dr", "me", "app"];
    let m = mot.trim_start_matches(['(', '«', '"', '\'']);
    // « M.B », « p.ex » : un point dans le mot ; « M », « J » : une initiale.
    m.contains('.') || m.chars().count() == 1 || ABREVIATIONS.contains(&m.to_lowercase().as_str())
}

/// Retire une marque de liste ou de titre en tête de ligne (`- `, `* `, `1. `, `## `, `> `).
fn sans_marque_de_liste(l: &str) -> &str {
    let l = l.trim_start_matches(['#', '>']).trim_start();
    for m in ["- ", "* ", "+ ", "• "] {
        if let Some(reste) = l.strip_prefix(m) {
            return reste;
        }
    }
    let chiffres = l.chars().take_while(char::is_ascii_digit).count();
    if chiffres > 0 && chiffres < 4 {
        let reste = &l[chiffres..];
        if let Some(r) = reste.strip_prefix(". ").or_else(|| reste.strip_prefix(") ")) {
            return r;
        }
    }
    l
}

/// Retire le gras, l'italique, le code en ligne et les liens (`[texte](adresse)` devient `texte`).
fn sans_markdown(l: &str) -> String {
    let mut s = String::with_capacity(l.len());
    let c: Vec<char> = l.chars().collect();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '*' | '`' => i += 1,
            '_' if c.get(i + 1) == Some(&'_') => i += 2,
            '[' => {
                // Un lien : on garde le texte, on saute l'adresse.
                if let Some(fin) = c[i..].iter().position(|&x| x == ']').map(|p| p + i)
                    && c.get(fin + 1) == Some(&'(')
                    && let Some(ferme) = c[fin..].iter().position(|&x| x == ')').map(|p| p + fin)
                {
                    s.extend(&c[i + 1..fin]);
                    i = ferme + 1;
                    continue;
                }
                s.push('[');
                i += 1;
            }
            x => {
                s.push(x);
                i += 1;
            }
        }
    }
    s
}

/// Ajoute un point à une ligne qui finit sans ponctuation, pour que la voix marque la pause.
fn ponctuer(l: &str) -> String {
    match l.chars().last() {
        Some('.' | '!' | '?' | ':' | ';' | '…' | ',') => l.to_string(),
        _ => format!("{l}."),
    }
}

/// Coupe un texte trop long à la dernière fin de phrase avant `max` caractères.
fn couper(texte: &str, max: usize) -> String {
    if texte.chars().count() <= max {
        return texte.to_string();
    }
    let debut: String = texte.chars().take(max).collect();
    let fin = debut.rfind(['.', '!', '?']).map(|p| p + 1).unwrap_or(debut.len());
    format!("{} Le reste est à l'écran.", debut[..fin].trim_end())
}

/// Minuscules, sans accents ni ponctuation, les espaces resserrées : la forme où l'on compare ce qui a été dit.
pub fn normaliser(s: &str) -> String {
    let mut n = String::with_capacity(s.len());
    for c in s.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'à' | 'â' | 'ä' | 'á' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' => 'i',
            'ô' | 'ö' | 'ó' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ç' => 'c',
            'œ' => {
                n.push_str("oe");
                continue;
            }
            c if c.is_alphanumeric() => c,
            _ => ' ',
        };
        n.push(c);
    }
    n.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Ce qui a été dit contient-il cette expression, en mots entiers ?
fn contient(n: &str, expression: &str) -> bool {
    format!(" {n} ").contains(&format!(" {expression} "))
}

/// La réponse à une demande d'accord : `Some(true)` pour « je confirme », `Some(false)` pour « annule » ou « non »,
/// `None` si ce n'est ni l'un ni l'autre (Eve redemande). Une réponse est courte : une phrase de plus de six mots
/// est une conversation (au téléphone, dans le bureau) ou la consigne d'Eve elle-même, qui porte les deux formules ;
/// et une phrase qui dit l'un et l'autre (« je confirme ou annule ») ne décide rien.
pub fn reponse_d_accord(dit: &str) -> Option<bool> {
    const NON: [&str; 10] = [
        "annule", "annuler", "non", "refuse", "arrete", "stop", "laisse tomber", "oublie ca", "pas question", "attends",
    ];
    let n = normaliser(dit);
    let mots = n.split_whitespace().count();
    if mots == 0 || mots > 6 {
        return None;
    }
    let refus = contient(&n, "confirme pas") || contient(&n, "ne confirme") || contient(&n, "pas confirme");
    let oui = !refus
        && (contient(&n, "je confirme") || contient(&n, "c est confirme") || n == "confirme" || n == "confirmer");
    let non = refus || NON.iter().any(|m| contient(&n, m));
    match (oui, non) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

/// La personne parle-t-elle du calendrier (« qu'est-ce que j'ai demain ? », « et mercredi ? ») ? Eve joint alors les
/// quinze jours qu'elle a relus : l'Adjointe répond sans relire la base.
pub fn parle_du_calendrier(dit: &str) -> bool {
    const MOTS: [&str; 22] = [
        "calendrier", "agenda", "horaire", "demain", "apres demain", "aujourd hui", "ce matin", "cet apres midi",
        "ce soir", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche", "semaine", "rendez vous",
        "rencontre", "prevu", "planifie", "qu est ce que j ai",
    ];
    let n = normaliser(dit);
    MOTS.iter().any(|m| contient(&n, m))
}

/// Un geste de la caméra d'Eve, dit à voix haute.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestePhoto {
    /// Ouvrir la caméra (« scanne cette facture », « prends une photo »).
    Ouvrir,
    /// Prendre la page devant la caméra (« photo », « page suivante »).
    Declic,
    /// Retirer la dernière page prise (« recommence »).
    Reprendre,
    /// Confier le document à l'Adjointe, pour qu'elle le classe (« classe-la », « c'est bon »).
    Classer,
    /// Fermer la caméra sans rien envoyer.
    Fermer,
}

impl GestePhoto {
    pub fn nom(self) -> &'static str {
        match self {
            GestePhoto::Ouvrir => "ouvrir",
            GestePhoto::Declic => "declic",
            GestePhoto::Reprendre => "reprendre",
            GestePhoto::Classer => "classer",
            GestePhoto::Fermer => "fermer",
        }
    }
}

/// Ce qu'on dit parfois avant un geste (« peux-tu… », « je veux… ») : le geste reste le même.
const AVANT_UN_GESTE: [&str; 14] = [
    "est ce que tu peux", "est ce que tu pourrais", "peux tu", "tu peux", "pourrais tu", "tu pourrais", "veux tu",
    "je veux", "je voudrais", "j aimerais", "on va", "il faut", "s il te plait", "ok",
];
/// Ce qui ouvre la caméra, en tête de phrase.
const OUVRIR: [&str; 22] = [
    "prends une photo", "prend une photo", "prendre une photo", "prends en photo", "prend en photo",
    "prendre en photo", "photographie", "photographier", "photo", "camera", "ouvre la camera", "ouvrir la camera",
    "ouvre ta camera", "allume la camera", "active la camera", "scanne", "scanner", "scan", "numerise", "numeriser",
    "fais un scan", "faire un scan",
];

/// La phrase commande-t-elle la caméra ?
///
/// Caméra fermée, seule l'ouverture s'entend, et seulement en tête de phrase (« scanne cette facture » ; « le scanner
/// est-il brisé ? » reste une question) : la phrase entière suit alors le document jusqu'à l'Adjointe, avec ce qu'elle
/// précise (« scanne la facture de Martin et inscris-la »).
///
/// Caméra ouverte, une phrase courte (sept mots au plus) : la photo, la dernière retirée, le classement, la fermeture.
pub fn geste_photo(dit: &str, camera_ouverte: bool) -> Option<GestePhoto> {
    let n = normaliser(dit);
    let mots = n.split_whitespace().count();
    if mots == 0 {
        return None;
    }
    let mut reste = n.as_str();
    for p in AVANT_UN_GESTE {
        if let Some(r) = commence_par(reste, p) {
            reste = r;
            break;
        }
    }
    let ouvre = OUVRIR.iter().any(|o| commence_par(reste, o).is_some());
    if !camera_ouverte {
        return ouvre.then_some(GestePhoto::Ouvrir);
    }
    if mots > 7 {
        return None;
    }
    let a = |liste: &[&str]| liste.iter().any(|m| contient(&n, m));
    if a(&["recommence", "reprends", "reprend", "efface", "supprime", "enleve", "retire", "derniere", "dernier"]) {
        Some(GestePhoto::Reprendre)
    } else if a(&[
        "classe", "range", "envoie", "envoyer", "c est bon", "termine", "terminer", "fini", "c est tout", "valide",
    ]) {
        Some(GestePhoto::Classer)
    } else if a(&["ferme", "fermer", "annule", "annuler", "laisse tomber", "oublie ca"]) {
        Some(GestePhoto::Fermer)
    } else if a(&[
        "photo", "clic", "clique", "vas y", "go", "capture", "suivante", "autre page", "encore", "prends", "prend",
    ]) {
        Some(GestePhoto::Declic)
    } else if ouvre {
        // « Ouvre la caméra » quand elle l'est déjà : la fenêtre revient devant, rien de plus.
        Some(GestePhoto::Ouvrir)
    } else {
        None
    }
}

/// La phrase dit-elle plus que le geste lui-même ? « Scanne la facture de Martin » précise ce qu'on attend ; « prends
/// une photo », « classe-la » ou « c'est bon » ne disent rien de plus à l'Adjointe.
pub fn precise_le_geste(dit: &str) -> bool {
    const SEULS: [&str; 12] = [
        "prends une photo", "prend une photo", "prendre une photo", "ouvre la camera", "c est bon", "c est tout",
        "c est fini", "j ai fini", "classe la", "classe le", "range la", "envoie la",
    ];
    let n = normaliser(dit);
    n.split_whitespace().count() >= 3 && !SEULS.contains(&n.as_str())
}

/// `n` commence-t-il par l'expression, en mots entiers ? Rend la suite.
fn commence_par<'a>(n: &'a str, expression: &str) -> Option<&'a str> {
    let r = n.strip_prefix(expression)?;
    if r.is_empty() { Some(r) } else { r.strip_prefix(' ') }
}

/// La personne demande-t-elle le point (« quoi de neuf ? ») ? Eve joint alors ce qu'elle a vu de la journée.
pub fn demande_les_nouvelles(dit: &str) -> bool {
    let n = normaliser(dit);
    ["quoi de neuf", "du nouveau", "quoi de nouveau", "des nouvelles", "les nouvelles"].iter().any(|e| contient(&n, e))
}

/// La personne demande-t-elle le résumé de la journée d'Eve (ce qu'elle a écrit, créé, modifié, supprimé) ?
pub fn demande_le_resume(dit: &str) -> bool {
    let n = normaliser(dit);
    [
        "resume de ma journee", "resume de la journee", "resume de ta journee", "bilan de la journee",
        "bilan de ta journee", "bilan de ma journee", "tu as fait aujourd hui", "as tu fait aujourd hui",
        "t as fait aujourd hui",
    ]
    .iter()
    .any(|e| contient(&n, e))
}

/// La personne demande-t-elle d'annuler la dernière modification de fichiers ? « Annule » seul, ou « annule ça »,
/// arrête ce qui est en cours ([`est_un_arret`]) : il faut dire ce qu'on annule.
pub fn demande_d_annuler(dit: &str) -> bool {
    let n = normaliser(dit);
    [
        "annule la derniere modification",
        "annule ta derniere modification",
        "annule la modification",
        "annule tes modifications",
        "annule le dernier changement",
        "annule tes changements",
        "annule ce que tu viens de faire",
        "defais ca",
        "defais le",
        "defais la derniere modification",
        "remets comme avant",
        "remets le comme avant",
        "remets la comme avant",
        "remets les comme avant",
        "reviens en arriere",
    ]
    .iter()
    .any(|e| contient(&n, e))
}

/// La personne demande-t-elle d'arrêter ce qui est en cours ? Une phrase courte seulement : « arrête de chercher les
/// factures de 2025 et donne-moi… » est une demande, pas un arrêt.
pub fn est_un_arret(dit: &str) -> bool {
    let n = normaliser(dit);
    let mots = n.split_whitespace().count();
    mots > 0 && mots <= 3 && ["arrete", "stop", "suffit", "silence", "tais", "annule"].iter().any(|m| contient(&n, m))
}

/// Les formes sous lesquelles Whisper rend « Eve », le mot d'éveil.
const EVE: [&str; 6] = ["eve", "eves", "ev", "eva", "hev", "heve"];
/// Les mots qu'on dit parfois avant d'appeler Eve.
const AVANT_EVE: [&str; 9] = ["hey", "he", "ok", "okay", "salut", "bonjour", "dis", "alors", "euh"];

/// La phrase appelle-t-elle Eve ? `Some(demande)` si elle commence par « Eve » (après un « hey », un « ok »…) : la
/// demande est le reste, tel qu'il a été dit, et peut être vide (« Eve ? » tout court) ; `None` sinon.
pub fn appel_a_eve(dit: &str) -> Option<String> {
    let mots: Vec<&str> = dit.split_whitespace().collect();
    let mut i = 0;
    while i < mots.len() && i < 3 {
        let n = normaliser(mots[i]);
        if EVE.contains(&n.as_str()) {
            let reste = mots[i + 1..].join(" ");
            let reste = reste.trim_start_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace()).trim();
            return Some(reste.to_string());
        }
        if !AVANT_EVE.contains(&n.as_str()) {
            return None;
        }
        i += 1;
    }
    None
}

/// Nettoie une transcription de Whisper : retire les annotations (`[Musique]`, `(rires)`) et rend une chaîne vide pour
/// ce que Whisper invente sur un silence.
pub fn nettoyer_transcription(texte: &str) -> String {
    let mut s = String::with_capacity(texte.len());
    let mut profondeur = 0usize;
    for c in texte.chars() {
        match c {
            '[' | '(' => profondeur += 1,
            ']' | ')' => profondeur = profondeur.saturating_sub(1),
            '*' => {}
            _ if profondeur == 0 => s.push(c),
            _ => {}
        }
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let n = normaliser(&s);
    if n.is_empty() || HALLUCINATIONS.iter().any(|h| n.contains(h)) {
        return String::new();
    }
    s
}

#[cfg(test)]
mod essais {
    #[test]
    fn le_resume_et_l_annulation_se_reconnaissent() {
        assert!(demande_le_resume("Eve, fais-moi le résumé de ma journée"));
        assert!(demande_le_resume("Qu'est-ce que tu as fait aujourd'hui ?"));
        assert!(!demande_le_resume("Quoi de neuf ?"));
        assert!(demande_d_annuler("Annule la dernière modification"));
        assert!(demande_d_annuler("Remets-le comme avant"));
        assert!(demande_d_annuler("Défais ça"));
        assert!(!demande_d_annuler("Annule ça"));
        assert!(est_un_arret("Annule ça"));
    }

    use super::*;

    #[test]
    fn seul_ce_qui_precede_les_tirets_se_dit() {
        let r = "Tu as **trois** factures à vérifier.\n\n---\n| nº | total |\n|---|---|\n| 15182 | 937,05 $ |";
        assert_eq!(pour_la_voix(r), "Tu as trois factures à vérifier.");
    }

    #[test]
    fn listes_liens_et_code() {
        let r = "Voici :\n- la [facture](https://x.ca/f) de Martin\n- le devis `Q-00735`\n```\ncode\n```\nFin";
        assert_eq!(pour_la_voix(r), "Voici : la facture de Martin. le devis Q-00735. Fin.");
    }

    #[test]
    fn une_reponse_longue_renvoie_a_l_ecran() {
        let r = "Une phrase assez longue pour remplir. ".repeat(40);
        let dit = pour_la_voix(&r);
        assert!(dit.ends_with("Le reste est à l'écran."));
        assert!(dit.chars().count() < LONGUEUR_MAX + 40);
    }

    #[test]
    fn l_accord_exige_je_confirme() {
        assert_eq!(reponse_d_accord("Je confirme."), Some(true));
        assert_eq!(reponse_d_accord("Oui, je confirme"), Some(true));
        assert_eq!(reponse_d_accord("Confirmé"), Some(true));
        assert_eq!(reponse_d_accord("Oui"), None);
        assert_eq!(reponse_d_accord("Non, annule."), Some(false));
        assert_eq!(reponse_d_accord("Je ne confirme pas"), Some(false));
        assert_eq!(reponse_d_accord("euh"), None);
    }

    /// La revue de robustesse du 2026-10-21 : la consigne d'Eve, une phrase qui dit les deux, une conversation.
    #[test]
    fn un_accord_ne_s_entend_pas_par_hasard() {
        let consigne = "J'ai besoin de ton accord pour inscrire une facture. Dis « je confirme », ou « annule ».";
        assert_eq!(reponse_d_accord(consigne), None);
        assert_eq!(reponse_d_accord("je confirme ou annule"), None);
        assert_eq!(reponse_d_accord("Oui, je confirme la livraison de jeudi avec le camion"), None);
        assert_eq!(reponse_d_accord("… je confirme ou annule. Annule"), None);
    }

    #[test]
    fn une_question_de_calendrier() {
        assert!(parle_du_calendrier("Qu'est-ce que j'ai demain ?"));
        assert!(parle_du_calendrier("Et mercredi ?"));
        assert!(parle_du_calendrier("C'est quoi le calendrier de la semaine prochaine"));
        assert!(!parle_du_calendrier("Classe cette facture au chantier Martin"));
    }

    #[test]
    fn les_gestes_de_la_camera() {
        use GestePhoto::*;
        // Caméra fermée : l'ouverture, en tête de phrase seulement.
        assert_eq!(geste_photo("Prends une photo", false), Some(Ouvrir));
        assert_eq!(geste_photo("Ouvre la caméra", false), Some(Ouvrir));
        assert_eq!(geste_photo("Scanne cette facture", false), Some(Ouvrir));
        assert_eq!(
            geste_photo("Peux-tu scanner la facture de Martin et l'inscrire dans la base ?", false),
            Some(Ouvrir)
        );
        assert_eq!(geste_photo("Je veux numériser un contrat", false), Some(Ouvrir));
        assert_eq!(geste_photo("Est-ce que le scanner fonctionne ?", false), None);
        assert_eq!(geste_photo("Le scan de Martin est-il arrivé ?", false), None);
        assert_eq!(geste_photo("Envoie la photo du balcon à Julie", false), None);
        assert_eq!(geste_photo("Classe ce document", false), None);
        assert_eq!(geste_photo("Photos du chantier ?", false), None);
        // Caméra ouverte : des phrases courtes.
        assert_eq!(geste_photo("Photo", true), Some(Declic));
        assert_eq!(geste_photo("Prends la photo", true), Some(Declic));
        assert_eq!(geste_photo("Prends une photo", true), Some(Declic));
        assert_eq!(geste_photo("Une autre page", true), Some(Declic));
        assert_eq!(geste_photo("Recommence", true), Some(Reprendre));
        assert_eq!(geste_photo("Reprends-la", true), Some(Reprendre));
        assert_eq!(geste_photo("Annule la dernière", true), Some(Reprendre));
        assert_eq!(geste_photo("Classe-la", true), Some(Classer));
        assert_eq!(geste_photo("C'est bon", true), Some(Classer));
        assert_eq!(geste_photo("Envoie-la à Julie", true), Some(Classer));
        assert_eq!(geste_photo("Ferme la caméra", true), Some(Fermer));
        assert_eq!(geste_photo("Laisse tomber", true), Some(Fermer));
        assert_eq!(geste_photo("Ouvre la caméra", true), Some(Ouvrir));
        assert_eq!(geste_photo("Qu'est-ce que j'ai demain ?", true), None);
        assert_eq!(geste_photo("Quelle est la date de la facture de Martin du mois dernier ?", true), None);
    }

    #[test]
    fn ce_qu_une_phrase_precise() {
        assert!(precise_le_geste("Scanne la facture de Martin"));
        assert!(precise_le_geste("Envoie-la à Julie"));
        assert!(!precise_le_geste("Prends une photo"));
        assert!(!precise_le_geste("Classe-la"));
        assert!(!precise_le_geste("C'est bon"));
        assert!(!precise_le_geste("Photo"));
    }

    #[test]
    fn quoi_de_neuf() {
        assert!(demande_les_nouvelles("Eve, quoi de neuf ?"));
        assert!(demande_les_nouvelles("Du nouveau ce matin ?"));
        assert!(!demande_les_nouvelles("Le calendrier de demain"));
    }

    /// Le texte donné morceau par morceau (`pas` caractères à la fois), comme il arrive de l'Adjointe.
    fn en_flux(texte: &str, pas: usize) -> Vec<String> {
        let mut f = EnFlux::default();
        let c: Vec<char> = texte.chars().collect();
        let mut phrases = Vec::new();
        for morceau in c.chunks(pas) {
            phrases.extend(f.ajouter(&morceau.iter().collect::<String>()));
        }
        phrases.extend(f.finir());
        phrases
    }

    #[test]
    fn la_parole_en_flux_dit_la_meme_chose_que_d_un_coup() {
        let textes = [
            "Demain, jeudi 22 octobre, tu as trois choses au calendrier. La démolition des jupettes A et B est confirmée.\n---\nDétail :\n- Démolition",
            "Tu as **trois** factures à vérifier.\n\n---\n| nº | total |\n|---|---|\n| 15182 | 937,05 $ |",
            "Voici :\n- la [facture](https://x.ca/f) de Martin\n- le devis `Q-00735`\n```\ncode\n```\nFin",
            "Toitures J.L. a reçu la facture de M. Tremblay. Elle est payée !\nC'est noté.",
            "---\nRien à dire avant les tirets ; ceci se dit.",
            "Bonjour Martin. Que puis-je faire pour toi ?",
        ];
        for t in textes {
            for pas in [1, 2, 3, 7, 1000] {
                assert_eq!(en_flux(t, pas).join(" "), pour_la_voix(t), "texte {t:?}, pas {pas}");
            }
        }
    }

    #[test]
    fn la_premiere_phrase_part_avant_la_fin() {
        // Le point se décide au mot qui suit : une majuscule, c'est une fin de phrase.
        let mut f = EnFlux::default();
        assert!(f.ajouter("Demain, tu as trois choses").is_empty());
        assert_eq!(f.ajouter(". La démolition"), vec!["Demain, tu as trois choses.".to_string()]);
        assert!(f.ajouter(" des").is_empty());
        let mut g = EnFlux::default();
        assert!(g.ajouter("C'est noté. ").is_empty());
        assert_eq!(g.ajouter("Je"), vec!["C'est noté.".to_string()]);
        // « J.L. » et « M. » ne coupent pas une phrase.
        let mut h = EnFlux::default();
        assert!(h.ajouter("Toitures J.L. a payé M. Tremblay hier. ").is_empty());
        assert_eq!(h.ajouter("Il"), vec!["Toitures J.L. a payé M. Tremblay hier.".to_string()]);
        // Après les trois tirets, plus rien.
        let mut k = EnFlux::default();
        let mut dit = k.ajouter("Fait.\n---\nDétail. Encore.");
        dit.extend(k.finir());
        assert_eq!(dit, vec!["Fait.".to_string()]);
    }

    #[test]
    fn la_parole_en_flux_s_arrete_au_bout() {
        let long = "Une phrase assez longue pour remplir. ".repeat(40);
        let dit = en_flux(&long, 5);
        assert_eq!(dit.last().map(String::as_str), Some("Le reste est à l'écran."));
        assert!(dit.iter().map(|p| p.chars().count()).sum::<usize>() < LONGUEUR_MAX + 40);
    }

    #[test]
    fn un_arret_est_une_phrase_courte() {
        assert!(est_un_arret("Arrête !"));
        assert!(est_un_arret("Stop."));
        assert!(!est_un_arret("Arrête de chercher les factures de 2025 et donne-moi celles d'octobre"));
    }

    #[test]
    fn la_transcription_se_nettoie() {
        assert_eq!(nettoyer_transcription(" [Musique] Bonjour Eve. "), "Bonjour Eve.");
        assert_eq!(nettoyer_transcription("Sous-titres réalisés par la communauté d'Amara.org"), "");
        assert_eq!(nettoyer_transcription("(rires)"), "");
    }

    #[test]
    fn eve_s_appelle_en_tete_de_phrase() {
        assert_eq!(
            appel_a_eve("Eve, qu'est-ce que j'ai cette semaine ?").as_deref(),
            Some("qu'est-ce que j'ai cette semaine ?")
        );
        assert_eq!(appel_a_eve("Ève ?").as_deref(), Some(""));
        assert_eq!(appel_a_eve("Hey Eve. Classe cette facture.").as_deref(), Some("Classe cette facture."));
        assert_eq!(appel_a_eve("OK Eva, arrête").as_deref(), Some("arrête"));
        assert_eq!(appel_a_eve("J'ai parlé à Eve hier"), None);
        assert_eq!(appel_a_eve("Je rêve d'un chantier"), None);
        assert_eq!(appel_a_eve(""), None);
    }

    #[test]
    fn normaliser_retire_accents_et_ponctuation() {
        assert_eq!(normaliser("Ça, c'est confirmé !"), "ca c est confirme");
    }
}
