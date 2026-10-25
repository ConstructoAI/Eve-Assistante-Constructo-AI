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
    /// Sans la limite de [`LONGUEUR_MAX`] : toutes les phrases, pour les pages de « la suite » ([`pages_pour_la_voix`]).
    illimite: bool,
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
        if !self.illimite && self.dit > 0 && self.dit + n > LONGUEUR_MAX {
            phrases.push("Le reste est à l'écran.".into());
            self.trop = true;
            return;
        }
        self.dit += n;
        self.dit_message = true;
        phrases.push(t);
    }
}

/// Ce que la voix peut lire d'une réponse, en pages de [`LONGUEUR_MAX`] caractères au plus, faites de phrases
/// entières : la première est ce que la voix en a dit ([`EnFlux`], qui s'arrête à la première phrase qui dépasserait) ;
/// les suivantes, ce qui en restait — la fin d'une réponse trop longue, puis le détail écrit après les trois tirets.
/// Les tableaux, les blocs de code et les cartes restent à l'écran. Pour « la suite » et « répète ».
pub fn pages_pour_la_voix(texte: &str) -> Vec<String> {
    // Le texte d'avant les trois tirets, et le détail d'après (des tirets en tête, avant tout texte, ne séparent rien).
    let (mut avant, mut apres, mut passe, mut dans_code) = (Vec::new(), Vec::new(), false, false);
    for ligne in texte.lines() {
        let l = ligne.trim();
        if l.starts_with("```") {
            dans_code = !dans_code;
        }
        if !passe && !dans_code && est_un_separateur(l) && avant.iter().any(|x: &&str| !x.trim().is_empty()) {
            passe = true;
            continue;
        }
        if passe { apres.push(ligne) } else { avant.push(ligne) }
    }
    let toutes = |lignes: &[&str]| {
        let mut f = EnFlux { illimite: true, ..EnFlux::default() };
        let mut phrases = f.ajouter(&lignes.join("\n"));
        phrases.extend(f.finir());
        phrases
    };
    let mut pages = paginer(toutes(&avant));
    pages.extend(paginer(toutes(&apres)));
    pages
}

/// Des phrases réunies en pages : une page prend des phrases tant qu'elle ne dépasse pas [`LONGUEUR_MAX`] caractères
/// (la règle de la voix en flux, qui compte ses phrases sans les espaces entre elles).
fn paginer(phrases: Vec<String>) -> Vec<String> {
    let (mut pages, mut page, mut n) = (Vec::new(), String::new(), 0);
    for p in phrases {
        let k = p.chars().count();
        if n > 0 && n + k > LONGUEUR_MAX {
            pages.push(std::mem::take(&mut page));
            n = 0;
        }
        if !page.is_empty() {
            page.push(' ');
        }
        page.push_str(&p);
        n += k;
    }
    if !page.is_empty() {
        pages.push(page);
    }
    pages
}

/// Ce qui a été dit (normalisé), sans le nom d'Eve en tête (« Eve, … » tapé, « ok Eve ») ni une politesse au bout
/// (« s'il te plaît », « stp ») : la forme où se reconnaissent les commandes courtes.
fn sans_politesse(n: &str) -> &str {
    let n = ["hey eve ", "ok eve ", "dis eve ", "eve "].iter().find_map(|p| n.strip_prefix(p)).unwrap_or(n);
    ["s il te plait", "s il vous plait", "stp", "svp", "merci"]
        .iter()
        .find_map(|p| n.strip_suffix(p).map(str::trim_end))
        .unwrap_or(n)
}

/// La personne demande-t-elle la suite de la réponse que la voix n'a pas finie (« la suite », « lis le reste ») ?
/// Jamais « continue » seul : on le dit à Claude pour qu'il poursuive un travail.
pub fn demande_la_suite(dit: &str) -> bool {
    let n = normaliser(dit);
    [
        "la suite", "suite", "et la suite", "lis la suite", "lis moi la suite", "dis la suite", "dis moi la suite",
        "continue la lecture", "continue de lire", "lis le reste", "lis moi le reste", "dis le reste", "le reste",
    ]
    .contains(&sans_politesse(&n))
}

/// La personne demande-t-elle de répéter ce que la voix vient de dire (« répète », « pardon ? ») ? Une phrase courte
/// seulement : « répète-moi les échéances » est une demande.
pub fn demande_de_repeter(dit: &str) -> bool {
    let n = normaliser(dit);
    [
        "repete", "repete ca", "repete le", "repete la", "tu peux repeter", "peux tu repeter", "redis le", "redis ca",
        "redis", "pardon", "quoi", "comment", "j ai pas compris", "je n ai pas compris", "j ai pas entendu",
        "je n ai pas entendu",
    ]
    .contains(&sans_politesse(&n))
}

/// La personne met-elle la sourdine (`Some(true)` : « sourdine », « mode silencieux ») ou la retire-t-elle
/// (`Some(false)` : « parle », « tu peux parler ») ? Des phrases courtes seulement : « parle-moi de la météo » est une
/// demande ; « silence », lui, arrête ce qui se dit ([`est_un_arret`]).
pub fn reglage_de_la_sourdine(dit: &str) -> Option<bool> {
    let n = normaliser(dit);
    let n = sans_politesse(&n);
    let mettre = ["sourdine", "en sourdine", "mets la sourdine", "mets toi en sourdine", "passe en sourdine"];
    let mettre_aussi = ["mode silencieux", "mode silence", "ecris seulement", "reponds par ecrit"];
    let retirer = [
        "parle", "parle moi", "reparle", "tu peux parler", "tu peux reparler", "parle a voix haute",
        "enleve la sourdine", "retire la sourdine", "sans sourdine", "plus de sourdine",
    ];
    if mettre.contains(&n) || mettre_aussi.contains(&n) {
        Some(true)
    } else if retirer.contains(&n) {
        Some(false)
    } else {
        None
    }
}

/// Quand sonne un rappel dit : dans tant de secondes, ou à une heure — de ce jour (demain si elle est passée), ou de
/// demain.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Quand {
    Dans(u32),
    A { heure: u32, minute: u32, demain: bool },
}

/// Un rappel dit, qu'Eve pose elle-même sans Claude : quand, et quoi rappeler (vide : une minuterie, ou un rappel sans
/// sujet).
#[derive(Debug, PartialEq)]
pub struct RappelDit {
    pub quand: Quand,
    pub texte: String,
    pub minuterie: bool,
}

/// Un mot de la phrase, normalisé ([`normaliser`]), et la place dans la phrase d'origine du mot écrit qui le porte
/// (« d'appeler » porte « d » et « appeler »).
type Mot = (String, usize, usize);

fn mots_places(dit: &str) -> Vec<Mot> {
    let mut mots = Vec::new();
    let mut debut = None;
    for (i, c) in dit.char_indices().chain(std::iter::once((dit.len(), ' '))) {
        match (c.is_whitespace(), debut) {
            (false, None) => debut = Some(i),
            (true, Some(d)) => {
                for m in normaliser(&dit[d..i]).split_whitespace() {
                    mots.push((m.to_string(), d, i));
                }
                debut = None;
            }
            _ => {}
        }
    }
    mots
}

/// Un nombre dit en chiffres ou en lettres (de un à soixante).
fn nombre(m: &str) -> Option<u32> {
    const LETTRES: [(&str, u32); 22] = [
        ("un", 1),
        ("une", 1),
        ("deux", 2),
        ("trois", 3),
        ("quatre", 4),
        ("cinq", 5),
        ("six", 6),
        ("sept", 7),
        ("huit", 8),
        ("neuf", 9),
        ("dix", 10),
        ("onze", 11),
        ("douze", 12),
        ("treize", 13),
        ("quatorze", 14),
        ("quinze", 15),
        ("seize", 16),
        ("vingt", 20),
        ("trente", 30),
        ("quarante", 40),
        ("cinquante", 50),
        ("soixante", 60),
    ];
    m.parse().ok().or_else(|| LETTRES.iter().find(|(l, _)| *l == m).map(|(_, n)| *n))
}

/// Une durée à partir du mot `i` (« 10 minutes », « une demi heure », « 1 heure et demie », « 2 h 15 ») : ses secondes,
/// et le mot qui la suit.
fn duree_dite(m: &[Mot], i: usize) -> Option<(u32, usize)> {
    let mot = |k: usize| m.get(k).map(|x| x.0.as_str());
    match (mot(i), mot(i + 1), mot(i + 2)) {
        (Some("une"), Some("demi"), Some("heure")) => return Some((1800, i + 3)),
        (Some("un"), Some("quart"), Some("d")) if mot(i + 3) == Some("heure") => return Some((900, i + 4)),
        _ => {}
    }
    let n = nombre(mot(i)?)?;
    let unite = match mot(i + 1)? {
        "seconde" | "secondes" | "sec" => 1,
        "minute" | "minutes" | "min" | "mn" => 60,
        "heure" | "heures" | "h" => 3600,
        _ => return None,
    };
    let (mut secondes, mut k) = (n * unite, i + 2);
    if unite == 3600 {
        match (mot(k), mot(k + 1)) {
            (Some("et"), Some("demie" | "demi")) => (secondes, k) = (secondes + 1800, k + 2),
            (Some("et"), Some("quart")) => (secondes, k) = (secondes + 900, k + 2),
            (Some(x), _) if nombre(x).is_some_and(|x| x < 60) => {
                secondes += nombre(x).unwrap_or(0) * 60;
                k += if matches!(mot(k + 1), Some("minute" | "minutes" | "min")) { 2 } else { 1 };
            }
            _ => {}
        }
    }
    (secondes > 0 && secondes <= 7 * 86_400).then_some((secondes, k))
}

/// Une heure à partir du mot `i` (« 16 h », « 16h30 », « 9 heures 15 », « 4 h de l'après-midi », « midi ») : l'heure,
/// la minute, et le mot qui la suit. Un nombre seul (« à 4 ») ne suffit pas : trop vague, il va à Claude.
fn heure_dite(m: &[Mot], i: usize) -> Option<(u32, u32, usize)> {
    let mot = |k: usize| m.get(k).map(|x| x.0.as_str());
    let (mut h, mut minute, mut k) = match mot(i)? {
        "midi" => (12, 0, i + 1),
        "minuit" => (0, 0, i + 1),
        // « 16h30 », « 16h » : en un seul mot (pas « huit »).
        x if x.starts_with(|c: char| c.is_ascii_digit()) && x.contains('h') => {
            let (a, b) = x.split_once('h')?;
            (a.parse().ok()?, if b.is_empty() { 0 } else { b.parse().ok()? }, i + 1)
        }
        x => {
            let h = nombre(x)?;
            match mot(i + 1)? {
                "h" | "heure" | "heures" => match mot(i + 2).and_then(nombre) {
                    Some(mi) if mi < 60 => (h, mi, i + 3),
                    _ => (h, 0, i + 2),
                },
                // « 16 30 » : « 16:30 », dont les deux-points sont tombés.
                y => (h, nombre(y).filter(|mi| *mi < 60)?, i + 2),
            }
        }
    };
    if matches!((mot(k), mot(k + 1)), (Some("et"), Some("demie" | "demi"))) {
        (minute, k) = (30, k + 2);
    }
    // « de l'après-midi », « du soir » : l'après-midi ; « du matin » : tel quel.
    let soir = [["de", "l", "apres", "midi"].as_slice(), &["du", "soir"]];
    for s in soir {
        if m.len() >= k + s.len() && m[k..k + s.len()].iter().map(|x| x.0.as_str()).eq(s.iter().copied()) {
            if h < 12 {
                h += 12;
            }
            k += s.len();
        }
    }
    if m.len() >= k + 2 && m[k].0 == "du" && m[k + 1].0 == "matin" {
        k += 2;
    }
    (h < 24 && minute < 60).then_some((h, minute, k))
}

/// Un moment dit à partir du mot `i` : « dans 10 minutes », « à 16 h », « demain à 9 h » ; et le mot qui le suit.
fn moment_dit(m: &[Mot], i: usize) -> Option<(Quand, usize)> {
    let mot = |k: usize| m.get(k).map(|x| x.0.as_str());
    match mot(i)? {
        "dans" => duree_dite(m, i + 1).map(|(s, k)| (Quand::Dans(s), k)),
        "a" => heure_dite(m, i + 1).map(|(heure, minute, k)| (Quand::A { heure, minute, demain: false }, k)),
        "demain" if mot(i + 1) == Some("a") => {
            heure_dite(m, i + 2).map(|(heure, minute, k)| (Quand::A { heure, minute, demain: true }, k))
        }
        _ => None,
    }
}

/// Ce qu'il faut rappeler : le texte d'origine des mots `de..a`, sans le mot qui l'introduit (« de », « d' »,
/// « que », « pour ») ni la ponctuation des bords.
fn sujet(dit: &str, m: &[Mot], de: usize, a: usize) -> String {
    if de >= a {
        return String::new();
    }
    // Jusqu'au mot `a`, ou jusqu'à la fin du dernier mot gardé (une politesse ôtée n'en est pas).
    let fin = if a < m.len() { m[a].1 } else { m.last().map_or(dit.len(), |x| x.2) };
    let mut t = dit[m[de].1..fin].trim();
    for lien in ["d'", "d’", "de ", "qu'", "qu’", "que ", "pour "] {
        if t.len() > lien.len() && t.get(..lien.len()).is_some_and(|x| x.eq_ignore_ascii_case(lien)) {
            t = t[lien.len()..].trim_start();
            break;
        }
    }
    t.trim_matches(|c: char| c.is_whitespace() || ",.;:!?".contains(c)).to_string()
}

/// Un rappel ou une minuterie dits, que la voie rapide pose sans Claude : « minuterie 10 minutes », « rappelle-moi
/// dans 20 minutes de sortir le gâteau », « rappelle-moi à 16 h d'appeler le plombier », « rappelle-moi d'appeler
/// Marie demain à 9 h ». Tout ce qui ne se lit pas aussi sûrement (« à 4 », « vendredi ») va à Claude, qui pose le
/// rappel par une carte.
pub fn rappel_dit(dit: &str) -> Option<RappelDit> {
    let mut m = mots_places(dit);
    // Une politesse au bout ne change rien (« …, s'il te plaît »).
    for p in [["s", "il", "te", "plait"].as_slice(), &["s", "il", "vous", "plait"], &["stp"], &["svp"], &["merci"]] {
        if m.len() > p.len() && m[m.len() - p.len()..].iter().map(|x| x.0.as_str()).eq(p.iter().copied()) {
            m.truncate(m.len() - p.len());
        }
    }
    let debut =
        m.iter().take(6).position(|x| matches!(x.0.as_str(), "minuterie" | "minuteur" | "rappelle" | "rappelles"))?;
    if matches!(m[debut].0.as_str(), "minuterie" | "minuteur") {
        let mut k = debut + 1;
        if m.get(k).is_some_and(|x| x.0 == "de" || x.0 == "d") {
            k += 1;
        }
        let (secondes, fin) = duree_dite(&m, k)?;
        // « minuterie de 10 minutes pour les pâtes » : son sujet.
        let texte = match m.get(fin) {
            None => String::new(),
            Some(x) if x.0 == "pour" => sujet(dit, &m, fin, m.len()),
            Some(_) => return None,
        };
        return Some(RappelDit { quand: Quand::Dans(secondes), texte, minuterie: true });
    }
    // « rappelle-moi », « rappelle-nous » : le moment avant ce qu'il faut rappeler, ou après.
    if !m.get(debut + 1).is_some_and(|x| x.0 == "moi" || x.0 == "nous") {
        return None;
    }
    let apres = debut + 2;
    if let Some((quand, k)) = moment_dit(&m, apres) {
        return Some(RappelDit { quand, texte: sujet(dit, &m, k, m.len()), minuterie: false });
    }
    // Le premier moment qui va jusqu'au bout : « de téléphoner à Marie demain à 9 h » (pas « à 9 h » seul).
    (apres + 1..m.len()).find_map(|k| {
        let (quand, fin) = moment_dit(&m, k)?;
        (fin == m.len()).then(|| RappelDit { quand, texte: sujet(dit, &m, apres, k), minuterie: false })
    })
}

/// Ce que vaut une transcription, avant de partir à Claude (le garde-fou de l'oreille).
#[derive(Debug, PartialEq)]
pub enum Transcription {
    /// Elle part : telle quelle, ou repliée (une boucle de Whisper, gardée une fois).
    Juste(String),
    /// Elle ne part pas : Eve dit qu'elle a mal entendu, et la met dans la saisie, à corriger ; la raison va au journal.
    Douteuse { texte: String, raison: &'static str },
    /// La voix d'Eve elle-même, revenue par le micro : elle s'ignore.
    MaVoix,
}

/// Ce que Whisper rend d'un bruit, d'une toux ou d'une hésitation : rien à demander.
const BRUITS: [&str; 21] = [
    "cough", "toux", "tousse", "hmm", "hm", "mmh", "mm", "mmm", "euh", "heu", "hum", "ah", "oh", "bah", "pff", "pfff",
    "rire", "rires", "musique", "applaudissements", "bruit",
];

/// Juge une transcription déjà nettoyée ([`nettoyer_transcription`]) avant qu'elle parte à Claude. `parole` et
/// `calcul` : ses secondes de parole et de calcul (0 : inconnues) ; `dites` : les phrases qu'Eve disait pendant que le
/// son se prenait (vide si elle se taisait : on répond souvent avec ses mots à elle). Les règles viennent du journal de
/// l'Assistante (une vingtaine de phrases brouillées en deux jours, dont une boucle de trois mots qui a fait modifier
/// un fichier) :
/// - la voix d'Eve : la phrase entière (trois mots au moins) dans une phrase qu'elle disait, ou presque toute (six mots
///   de suite au moins, les quatre cinquièmes de la phrase) ;
/// - un bruit seul (« Cough », « euh »), une seule lettre, ou des lettres d'une autre écriture que la latine ;
/// - une boucle (deux mots ou plus, répétés trois fois de suite ; un mot, quatre fois) se replie : douteuse s'il n'en
///   reste que trois mots ou moins ;
/// - plus de cinq mots par seconde de parole (on en dit trois au plus) ;
/// - un calcul de plus de six secondes et de plus du triple de la parole (une boucle qui n'a pas encore été repliée).
pub fn juger_la_transcription(
    texte: &str,
    parole: f32,
    calcul: f32,
    sur_la_carte: bool,
    dites: &[String],
) -> Transcription {
    let douteuse = |raison| Transcription::Douteuse { texte: texte.to_string(), raison };
    let n = normaliser(texte);
    let mots: Vec<&str> = n.split_whitespace().collect();
    if mots.len() >= 3 {
        for d in dites {
            let d = normaliser(d);
            let commune = plus_longue_suite_commune(&mots, &d.split_whitespace().collect::<Vec<_>>());
            if contient(&d, &n) || (commune >= 6 && commune as f32 >= 0.8 * mots.len() as f32) {
                return Transcription::MaVoix;
            }
        }
    }
    if mots.is_empty() || BRUITS.contains(&n.as_str()) || (mots.len() == 1 && n.chars().count() == 1) {
        return douteuse("un bruit, rien de clair");
    }
    let latin = |c: char| c.is_ascii() || ('\u{00C0}'..='\u{024F}').contains(&c) || "’«»—–…".contains(c);
    if texte.chars().any(|c| c.is_alphabetic() && !latin(c)) {
        return douteuse("une autre écriture");
    }
    let replie = replier_les_boucles(texte);
    let plie = replie.split_whitespace().count() < texte.split_whitespace().count();
    let mots_replies = normaliser(&replie).split_whitespace().count();
    let repliee = |raison| Transcription::Douteuse { texte: replie.clone(), raison };
    if plie && mots_replies <= 3 {
        return repliee("une boucle de l'oreille");
    }
    if parole >= 1.0 && mots_replies as f32 / parole > 5.0 {
        return repliee("trop de mots pour sa durée");
    }
    if sur_la_carte && !plie && calcul > 6.0 && calcul > 3.0 * parole && parole > 0.0 {
        return douteuse("l'oreille a peiné");
    }
    Transcription::Juste(if plie { replie } else { texte.trim().to_string() })
}

/// La plus longue suite de mots que `a` et `b` ont en commun, telle quelle.
fn plus_longue_suite_commune(a: &[&str], b: &[&str]) -> usize {
    let mut meilleure = 0;
    for i in 0..a.len() {
        for j in 0..b.len() {
            let k = a[i..].iter().zip(&b[j..]).take_while(|(x, y)| x == y).count();
            meilleure = meilleure.max(k);
        }
    }
    meilleure
}

/// Une transcription où Whisper a bouclé (« 2, ok. 2, ok. 2, ok. ») : chaque suite de deux mots ou plus répétée trois
/// fois de suite (un mot, quatre fois), gardée une fois. Les mots se comparent normalisés ; ceux qui restent gardent
/// leur forme.
fn replier_les_boucles(texte: &str) -> String {
    let mut mots: Vec<&str> = texte.split_whitespace().collect();
    loop {
        let normes: Vec<String> = mots.iter().map(|m| normaliser(m)).collect();
        let mut repli = None;
        'cherche: for taille in 1..=8.min(mots.len() / 3) {
            let fois_min = if taille == 1 { 4 } else { 3 };
            for debut in 0..mots.len() {
                let mut fois = 1;
                while debut + (fois + 1) * taille <= mots.len()
                    && normes[debut + fois * taille..debut + (fois + 1) * taille] == normes[debut..debut + taille]
                {
                    fois += 1;
                }
                if fois >= fois_min && normes[debut..debut + taille].iter().any(|m| !m.is_empty()) {
                    repli = Some((debut, taille, fois));
                    break 'cherche;
                }
            }
        }
        let Some((debut, taille, fois)) = repli else { break };
        mots.drain(debut + taille..debut + fois * taille);
    }
    mots.join(" ")
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

/// La personne demande-t-elle le point du jour (« Eve, le point », « fais-moi le point ») ? Eve le fait elle-même,
/// d'après son tableau de bord, sans Claude.
pub fn demande_le_point(dit: &str) -> bool {
    let n = normaliser(dit);
    [
        "le point",
        "point",
        "fais le point",
        "fais moi le point",
        "le point du jour",
        "point du jour",
        "fais moi le point du jour",
        "fais moi le point de la journee",
        "le point de la journee",
        "mon point du jour",
    ]
    .contains(&sans_politesse(&n))
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

/// Ce qu'Eve dit d'une panne de Claude, d'après sa sorte (le champ `error` de Claude Code, ou « reseau » quand l'API
/// ne répond pas du tout) : à l'écran ce qu'il faut faire, à voix haute l'essentiel. `reprise` : l'heure où la limite
/// se rouvre (« 15 h 20 »), si elle est connue ; `sur_opus` : Sonnet est une issue (on le choisit dans les réglages,
/// sans Claude). `None` : une sorte qu'Eve ne sait pas dire mieux que « il y a eu un problème ».
pub fn phrase_de_panne(panne: &str, reprise: Option<&str>, sur_opus: bool) -> Option<(String, String)> {
    let pareil = |t: &str| (t.to_string(), t.to_string());
    Some(match panne {
        "authentication_failed" | "oauth_org_not_allowed" => (
            "Claude Code n'est plus connecté à ton compte. Pour le reconnecter : ouvre un terminal, tape « claude », \
             puis « /login »."
                .to_string(),
            "Claude Code n'est plus connecté à ton compte. Le détail est à l'écran.".to_string(),
        ),
        "billing_error" => pareil("Ton compte Claude a un problème de facturation : vérifie-le sur claude.ai."),
        "account_on_hold" => pareil("Ton compte Claude est suspendu : vérifie-le sur claude.ai."),
        "verification_required" => pareil("Claude demande une vérification de ton compte, sur claude.ai."),
        "rate_limit" => {
            let quand = reprise.map(|r| format!(" jusqu'à {r}")).unwrap_or_default();
            let issue = if sur_opus { " Tu peux choisir Sonnet dans Réglages, Cerveau." } else { "" };
            pareil(&format!("Ma limite d'utilisation de Claude est atteinte{quand}.{issue}"))
        }
        "overloaded" => pareil("Les serveurs de Claude sont surchargés. Redemande-moi dans un moment."),
        "server_error" => pareil("Claude a un problème de son côté. Redemande-moi dans un moment."),
        "model_not_found" => (
            "Le modèle choisi n'est pas disponible : choisis Sonnet dans Réglages › Cerveau.".to_string(),
            "Le modèle choisi n'est pas disponible. Le détail est à l'écran.".to_string(),
        ),
        "max_output_tokens" => pareil("Ma réponse était trop longue : elle a été coupée."),
        "reseau" => pareil("Je n'arrive pas à joindre Claude : vérifie la connexion Internet."),
        _ => return None,
    })
}

/// La personne demande-t-elle ce qu'Eve a coûté (« combien tu m'as coûté aujourd'hui ? ») ? Eve répond elle-même,
/// d'après son journal. Elle doit parler d'Eve ou de Claude : « combien m'a coûté la toiture ? » est une demande.
pub fn demande_le_cout(dit: &str) -> bool {
    let n = normaliser(dit);
    n.split_whitespace().count() <= 12
        && [
            "combien tu m as coute", "combien m as tu coute", "combien tu as coute", "combien tu me coutes",
            "combien tu coutes", "combien me coutes tu", "combien coutes tu", "ce que tu m as coute",
            "ton cout du jour", "ton cout aujourd hui", "combien claude m a coute", "combien claude a coute",
            "combien a coute claude", "combien m a coute claude", "cout de claude", "couts de claude",
            "depense en claude", "depenses en claude",
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

/// L'émotion d'une réponse, que montre le visage d'Eve (son portrait) : Claude la met en tête de sa réponse, entre
/// crochets (« [triste] »), et Eve l'ôte avant de la dire et de l'afficher ([`emotion_en_tete`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Emotion {
    Joie,
    Amusee,
    Empathie,
    Triste,
    Surprise,
    Reflexion,
    Serieuse,
    Neutre,
}

impl Emotion {
    /// Toutes, dans l'ordre où la consigne les donne à Claude.
    pub const TOUTES: [Emotion; 8] = [
        Emotion::Joie,
        Emotion::Amusee,
        Emotion::Empathie,
        Emotion::Triste,
        Emotion::Surprise,
        Emotion::Reflexion,
        Emotion::Serieuse,
        Emotion::Neutre,
    ];

    /// Son nom, tel que Claude l'écrit et que s'appelle sa pose (`portrait-emotion-triste.png`).
    pub fn nom(self) -> &'static str {
        match self {
            Emotion::Joie => "joie",
            Emotion::Amusee => "amusee",
            Emotion::Empathie => "empathie",
            Emotion::Triste => "triste",
            Emotion::Surprise => "surprise",
            Emotion::Reflexion => "reflexion",
            Emotion::Serieuse => "serieuse",
            Emotion::Neutre => "neutre",
        }
    }

    /// L'émotion d'un mot, sans égard aux accents ni à la casse, et ses proches (« amusée », « drôle », « sérieux »,
    /// « tristesse », « joyeuse ») ; `None` : ce n'en est pas une.
    pub fn de(mot: &str) -> Option<Emotion> {
        let m = normaliser(mot);
        let m = m.trim();
        let racines: [(&str, Emotion); 10] = [
            ("joi", Emotion::Joie),
            ("joy", Emotion::Joie),
            ("amus", Emotion::Amusee),
            ("drol", Emotion::Amusee),
            ("empath", Emotion::Empathie),
            ("trist", Emotion::Triste),
            ("surpri", Emotion::Surprise),
            ("reflex", Emotion::Reflexion),
            ("serieu", Emotion::Serieuse),
            ("neutr", Emotion::Neutre),
        ];
        (!m.contains(' ')).then_some(())?;
        racines.iter().find(|(r, _)| m.starts_with(r)).map(|&(_, e)| e)
    }
}

/// L'émotion en tête d'un texte (« [triste] Je suis désolée… ») et le texte sans elle ; sans émotion reconnue en tête,
/// le texte tel quel.
pub fn emotion_en_tete(texte: &str) -> (Option<Emotion>, &str) {
    let t = texte.trim_start();
    if let Some(reste) = t.strip_prefix('[')
        && let Some(fin) = reste.find(']')
        && let Some(e) = Emotion::de(&reste[..fin])
    {
        return (Some(e), reste[fin + 1..].trim_start());
    }
    (None, texte)
}

/// Le début d'une réponse qui arrive en morceaux, examiné pour son émotion en tête : `None` tant qu'il peut encore en
/// être une (rien encore, ou un « [ » sans son « ] » dans ses vingt premiers caractères), et tant que rien ne suit
/// l'émotion (le texte qui viendra commencera sans blanc) ; puis l'émotion, s'il y en a une, et le texte qui la suit (le
/// début tel quel, sinon).
pub fn emotion_du_debut(debut: &str) -> Option<(Option<Emotion>, &str)> {
    let t = debut.trim_start();
    if t.is_empty() || (t.starts_with('[') && !t.contains(']') && t.chars().count() <= 20) {
        return None;
    }
    match emotion_en_tete(debut) {
        (Some(_), "") => None,
        r => Some(r),
    }
}

/// La forme de la bouche sur une syllabe (d'après sa voyelle), pour le portrait d'Eve : ouverte (a, è, an, oi), ronde
/// (o, ou, u, eu, on ; le « e » d'un mot d'une syllabe), étirée (i, é, ai, in, un) ; fermée, les lèvres closes, avant
/// m, b, p.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forme {
    Ouverte,
    Ronde,
    Etiree,
    Fermee,
}

/// Une syllabe telle que la bouche la dit : la forme de sa voyelle, et si les lèvres se ferment avant elle (un m, un b
/// ou un p la précède).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Syllabe {
    pub forme: Forme,
    pub fermee_avant: bool,
}

/// Les voyelles du français écrit.
const VOYELLES: &str = "aàâäeéèêëiîïoôöuùûüyœæ";

/// Les syllabes d'un texte, telles que la bouche les dit : chaque groupe de voyelles d'un mot en est une, sa forme
/// d'après ses lettres ([`forme_de`]) ; le « e » de fin d'un mot de plusieurs syllabes (« -e », « -es ») ne se dit pas,
/// ni le « u » de « qu », ni celui de « gu » devant e ou i ; les lèvres se ferment avant une syllabe qu'un m, un b ou un
/// p précède (« pr », « mp » compris ; pas « ph »). Une approximation du français écrit, pour la bouche et non pour
/// l'oreille : un chiffre ou un sigle n'y a pas de syllabe, et la piste de la voix répartit ce qui manque.
pub fn syllabes(texte: &str) -> Vec<Syllabe> {
    let voyelle = |c: char| VOYELLES.contains(c);
    let mut sortie = Vec::new();
    for mot in texte.split(|c: char| !c.is_alphabetic()).filter(|m| !m.is_empty()) {
        let l: Vec<char> = mot.to_lowercase().chars().collect();
        // Les groupes de voyelles du mot : leur début et leur fin (exclue).
        let mut groupes = Vec::new();
        let mut i = 0;
        while i < l.len() {
            if !voyelle(l[i]) {
                i += 1;
                continue;
            }
            let devant_e_ou_i = l.get(i + 1).is_some_and(|&c| "eéèêiîy".contains(c));
            let u_muet = l[i] == 'u' && i > 0 && (l[i - 1] == 'q' || (l[i - 1] == 'g' && devant_e_ou_i));
            let mut fin = i + 1;
            while fin < l.len() && voyelle(l[fin]) {
                fin += 1;
            }
            let debut = if u_muet { i + 1 } else { i };
            if debut < fin {
                groupes.push((debut, fin));
            }
            i = fin;
        }
        let n = groupes.len();
        let mut precedente = 0;
        for (k, &(debut, fin)) in groupes.iter().enumerate() {
            let consonnes = &l[precedente..debut];
            precedente = fin;
            let g: String = l[debut..fin].iter().collect();
            let suite = &l[fin..];
            if g == "e" && k + 1 == n && n > 1 && matches!(suite, [] | ['s']) {
                continue;
            }
            // Nasale : un n ou un m, puis une consonne ou la fin du mot (ni « nn », ni « mm », ni une voyelle).
            let nasale = matches!(suite.first(), Some('n' | 'm'))
                && suite.get(1).is_none_or(|&c| !voyelle(c) && c != 'n' && c != 'm');
            let fermee_avant = consonnes
                .iter()
                .enumerate()
                .any(|(j, &c)| matches!(c, 'm' | 'b' | 'p') && consonnes.get(j + 1) != Some(&'h'));
            sortie.push(Syllabe { forme: forme_de(&g, nasale, suite, n), fermee_avant });
        }
    }
    sortie
}

/// La forme de la bouche pour un groupe de voyelles `g` : `nasale`, suivi d'un n ou d'un m qui se dit avec lui ;
/// `suite`, les lettres qui le suivent dans le mot ; `n`, les syllabes du mot.
fn forme_de(g: &str, nasale: bool, suite: &[char], n: usize) -> Forme {
    use Forme::{Etiree, Ouverte, Ronde};
    let commence = |prefixes: &[&str]| prefixes.iter().any(|p| g.starts_with(p));
    if ["ou", "oû", "où"].iter().any(|p| g.contains(p)) {
        Ronde
    } else if commence(&["oi", "oî"]) {
        Ouverte
    } else if g.contains("au") || g.contains("eu") || commence(&["o", "ô", "ö", "œ"]) {
        Ronde
    } else if commence(&["ui"]) {
        Etiree
    } else if commence(&["u", "û", "ü", "ù"]) {
        // « un » : une nasale étirée.
        if nasale { Etiree } else { Ronde }
    } else if commence(&["ai", "aî", "ei", "é", "i", "î", "ï", "y"]) {
        Etiree
    } else if commence(&["e"]) {
        // « en », « em » : ouverte ; « -er », « -ez », « -et » en fin de mot, « les », « des » : é ; le « e » d'un mot
        // d'une syllabe (« le », « de », « que ») : un peu arrondi ; sinon (« elle », « merci ») : è.
        match suite {
            _ if nasale => Ouverte,
            ['r'] | ['z'] | ['t'] => Etiree,
            ['s'] if n == 1 => Etiree,
            [] if n == 1 => Ronde,
            _ => Ouverte,
        }
    } else {
        Ouverte
    }
}

#[cfg(test)]
mod essais {
    #[test]
    fn l_emotion_en_tete_s_ote() {
        use super::{Emotion, emotion_du_debut, emotion_en_tete};
        assert_eq!(emotion_en_tete("[triste] Je suis désolée."), (Some(Emotion::Triste), "Je suis désolée."));
        assert_eq!(emotion_en_tete("  [Amusée]Ha ! Bonne blague."), (Some(Emotion::Amusee), "Ha ! Bonne blague."));
        assert_eq!(emotion_en_tete("[sérieux] Parlons du budget."), (Some(Emotion::Serieuse), "Parlons du budget."));
        // Sans émotion reconnue, le texte tel quel : un lien, une note entre crochets, une émotion qui n'est pas en tête.
        let lien = "[Voir le plan](https://exemple.ca) ici";
        assert_eq!(emotion_en_tete(lien), (None, lien));
        assert_eq!(emotion_en_tete("Bonjour [joie]"), (None, "Bonjour [joie]"));
        // En morceaux : on attend le « ] » ; un « [ » trop long n'ouvre pas une émotion.
        assert_eq!(emotion_du_debut(""), None);
        assert_eq!(emotion_du_debut("[tri"), None);
        // L'émotion seule attend son texte : celui qui vient en morceaux après elle ne commence pas par un blanc.
        assert_eq!(emotion_du_debut("[triste]"), None);
        assert_eq!(emotion_du_debut("[triste] \n"), None);
        assert_eq!(emotion_du_debut("[triste] Je"), Some((Some(Emotion::Triste), "Je")));
        assert_eq!(emotion_du_debut("Bonjour"), Some((None, "Bonjour")));
        let long = "[Ceci est une note bien trop longue";
        assert_eq!(emotion_du_debut(long), Some((None, long)));
        // Chaque émotion se relit par son nom.
        for e in Emotion::TOUTES {
            assert_eq!(Emotion::de(e.nom()), Some(e));
        }
    }

    #[test]
    fn la_bouche_lit_les_syllabes() {
        use super::{Forme, syllabes};
        use Forme::{Etiree as I, Ouverte as A, Ronde as O};
        let formes = |t: &str| syllabes(t).iter().map(|s| (s.forme, s.fermee_avant)).collect::<Vec<_>>();
        assert_eq!(formes("Bonjour"), [(O, true), (O, false)]);
        assert_eq!(formes("merci"), [(A, true), (I, false)]);
        assert_eq!(formes("papa"), [(A, true), (A, true)]);
        assert_eq!(formes("maison"), [(I, true), (O, false)]);
        // Le « e » muet de fin de mot ne se dit pas ; celui d'un mot d'une syllabe, si.
        assert_eq!(formes("elle"), [(A, false)]);
        assert_eq!(formes("que le"), [(O, false), (O, false)]);
        assert_eq!(formes("les"), [(I, false)]);
        // « qu », « gu » : le u ne se dit pas ; « ph » n'est pas un p ; « pr » ferme les lèvres.
        assert_eq!(formes("guerre"), [(A, false)]);
        assert_eq!(formes("téléphone"), [(I, false), (I, false), (O, false)]);
        assert_eq!(formes("prévus"), [(I, true), (O, false)]);
        // Les nasales : « on » ronde, « en » ouverte, « un » étirée ; « bonne » n'en est pas une.
        assert_eq!(formes("temps"), [(A, false)]);
        assert_eq!(formes("un"), [(I, false)]);
        assert_eq!(formes("bonne"), [(O, true)]);
        assert_eq!(formes("aujourd'hui"), [(O, false), (O, false), (I, false)]);
        // Un chiffre n'a pas de syllabe ici.
        assert_eq!(formes("Voici 240 $."), [(A, false), (I, false)]);
        assert!(syllabes("240 $ — !").is_empty());
    }

    #[test]
    fn les_rappels_dits_se_lisent() {
        let r = |t: &str| rappel_dit(t).unwrap_or_else(|| panic!("« {t} »"));
        let a = |heure, minute, demain| Quand::A { heure, minute, demain };
        assert_eq!(
            r("Minuterie 10 minutes."),
            RappelDit { quand: Quand::Dans(600), texte: String::new(), minuterie: true }
        );
        assert_eq!(r("Mets une minuterie de cinq minutes, s'il te plaît").quand, Quand::Dans(300));
        let pates = r("Minuterie de 12 minutes pour les pâtes");
        assert_eq!((pates.quand, pates.texte.as_str()), (Quand::Dans(720), "les pâtes"));
        let g = r("Rappelle-moi dans 20 minutes de sortir le gâteau du four.");
        assert_eq!((g.quand, g.texte.as_str(), g.minuterie), (Quand::Dans(1200), "sortir le gâteau du four", false));
        let p = r("Eve, rappelle-moi à 16 h d'appeler le plombier");
        assert_eq!((p.quand, p.texte.as_str()), (a(16, 0, false), "appeler le plombier"));
        let m = r("Rappelle-moi de téléphoner à Marie demain à 9 h 30, s'il te plaît");
        assert_eq!((m.quand, m.texte.as_str()), (a(9, 30, true), "téléphoner à Marie"));
        let m = r("Rappelle-moi de téléphoner à Marie à 16 h");
        assert_eq!((m.quand, m.texte.as_str()), (a(16, 0, false), "téléphoner à Marie"));
        assert_eq!(r("rappelle-moi à 16h30 que le camion arrive").texte, "le camion arrive");
        assert_eq!(r("Rappelle-moi à 4 h de l'après-midi de passer chez Pierre").quand, a(16, 0, false));
        assert_eq!(r("Rappelle-moi à 4 h de l'après-midi de passer chez Pierre").texte, "passer chez Pierre");
        assert_eq!(r("Rappelle-moi dans une demi-heure").quand, Quand::Dans(1800));
        assert_eq!(r("rappelle-moi dans 1 heure et demie de rappeler le client").quand, Quand::Dans(5400));
        assert_eq!(r("Rappelle-moi à huit heures de sortir le chien").quand, a(8, 0, false));
        assert_eq!(r("Rappelle-moi à midi de manger").quand, a(12, 0, false));
        // Ce qui ne se lit pas aussi sûrement va à Claude.
        for t in [
            "Rappelle-moi à 4 de sortir le chien",
            "Rappelle-moi vendredi d'appeler le notaire",
            "Quelle heure est-il ?",
            "Peux-tu me rappeler demain ?",
            "Minuterie",
            "Rappelle-moi le nom du plombier",
            "Minuterie de 10 minutes et après fais le café",
        ] {
            assert_eq!(rappel_dit(t), None, "« {t} »");
        }
    }

    #[test]
    fn le_garde_fou_de_l_oreille() {
        let juste = |t: &str| Transcription::Juste(t.to_string());
        let doute = |t: &str| match juger_la_transcription(t, 3.0, 0.8, true, &[]) {
            Transcription::Douteuse { raison, .. } => raison,
            autre => panic!("« {t} » : {autre:?}"),
        };
        // Les phrases ordinaires passent, les courtes aussi.
        for t in ["Oui", "Non merci", "Ajoute du lait à l'épicerie.", "Qu'est-ce que j'ai demain ?"] {
            assert_eq!(juger_la_transcription(t, 2.0, 0.6, true, &[]), juste(t), "{t}");
        }
        // Une boucle se replie ; douteuse s'il n'en reste presque rien (la forme du journal : « 2, ok. » trois fois).
        assert_eq!(doute("2, ok. 2, ok. 2, ok."), "une boucle de l'oreille");
        assert_eq!(
            juger_la_transcription(
                "Ajoute du lait à l'épicerie. Ajoute du lait à l'épicerie. Ajoute du lait à l'épicerie.",
                3.0,
                9.0,
                true,
                &[]
            ),
            juste("Ajoute du lait à l'épicerie.")
        );
        // Un bruit, une lettre, une autre écriture.
        assert_eq!(doute("Cough"), "un bruit, rien de clair");
        assert_eq!(doute("Euh..."), "un bruit, rien de clair");
        assert_eq!(doute("E."), "un bruit, rien de clair");
        assert_eq!(doute("Спасибо"), "une autre écriture");
        // Quarante mots en 2,8 s : inventés.
        let quarante = ["mot"; 40].iter().enumerate().map(|(k, m)| format!("{m}{k}")).collect::<Vec<_>>().join(" ");
        assert_eq!(
            juger_la_transcription(&quarante, 2.8, 1.0, true, &[]),
            Transcription::Douteuse { texte: quarante.clone(), raison: "trop de mots pour sa durée" }
        );
        // Une oreille qui a peiné (11 s de calcul pour 2,6 s de parole) : sur la carte, une boucle ; sur le processeur,
        // une phrase courte en prend déjà sept (le journal du 23 octobre, avant la carte).
        assert!(matches!(
            juger_la_transcription("Dis-moi la météo de demain", 2.6, 11.2, true, &[]),
            Transcription::Douteuse { raison: "l'oreille a peiné", .. }
        ));
        assert_eq!(juger_la_transcription("C'est Léa", 2.5, 8.9, false, &[]), juste("C'est Léa"));
        // La voix d'Eve, revenue par le micro : la phrase entière dans ce qu'elle disait, ou presque toute.
        let dites = vec!["Je m'en occupe, je regarde ton agenda de la semaine.".to_string()];
        assert_eq!(juger_la_transcription("je regarde ton agenda", 1.5, 0.5, true, &dites), Transcription::MaVoix);
        assert_eq!(
            juger_la_transcription("Je regarde ton agenda de la semaine prochaine", 3.0, 0.8, true, &dites),
            Transcription::MaVoix
        );
        assert_eq!(
            juger_la_transcription("Ton agenda", 1.0, 0.4, true, &dites),
            juste("Ton agenda"),
            "deux mots : trop peu"
        );
        // Une réponse qui reprend ses mots n'est pas sa voix.
        let dites = vec!["Veux-tu que je l'ajoute à ta liste d'épicerie ?".to_string()];
        let reponse = "Oui, ajoute-le à ta liste d'épicerie";
        assert_eq!(juger_la_transcription(reponse, 2.2, 0.6, true, &dites), juste(reponse));
        assert_eq!(
            replier_les_boucles("très très très bien"),
            "très très très bien",
            "un mot, trois fois : du français"
        );
    }

    /// Le garde-fou sur un vrai journal (`EVE_JOURNAL` : son chemin) : ce qu'il aurait fait de chaque transcription.
    /// `cargo test --release --lib -- --ignored le_garde_fou_sur_un_journal --nocapture`.
    #[test]
    #[ignore]
    fn le_garde_fou_sur_un_journal() {
        let Some(chemin) = std::env::var_os("EVE_JOURNAL") else { return };
        let journal = String::from_utf8_lossy(&std::fs::read(chemin).unwrap_or_default()).into_owned();
        let (mut justes, mut repliees, mut douteuses, mut carte) = (0, 0, 0, false);
        for ligne in journal.lines() {
            if ligne.contains(" oreille : Whisper ") {
                carte = ligne.contains("sur la carte graphique");
            }
            let Some((_, apres)) = ligne.split_once(" entendu (") else { continue };
            let Some((mesures, texte)) = apres.split_once(") : ") else { continue };
            let Some((parole, calcul)) = mesures.split_once(" s de parole, ") else { continue };
            let (parole, calcul) =
                (parole.parse().unwrap_or(0.0), calcul.trim_end_matches(" s").parse().unwrap_or(0.0));
            let t = nettoyer_transcription(texte);
            if t.is_empty() {
                continue;
            }
            match juger_la_transcription(&t, parole, calcul, carte, &[]) {
                Transcription::Juste(j) if j == t => justes += 1,
                Transcription::Juste(j) => {
                    repliees += 1;
                    println!("repliée ({parole} s, {calcul} s) : {t}\n    → {j}");
                }
                Transcription::Douteuse { raison, .. } => {
                    douteuses += 1;
                    println!("douteuse, {raison} ({parole} s, {calcul} s) : {t}");
                }
                Transcription::MaVoix => {}
            }
        }
        println!("{justes} justes, {repliees} repliées, {douteuses} douteuses");
    }

    #[test]
    fn la_suite_reprend_ou_la_voix_s_est_arretee() {
        let phrases: Vec<String> = (1..=30).map(|k| format!("Voici la phrase numéro {k} de la réponse.")).collect();
        let texte =
            format!("{}\n\n| a | b |\n|---|---|\n| 1 | 2 |\n---\nLe détail : une note écrite.", phrases.join(" "));
        let pages = pages_pour_la_voix(&texte);
        // La première page est ce que la voix a dit en flux, sans « Le reste est à l'écran. ».
        let mut f = EnFlux::default();
        let mut dit = f.ajouter(&texte);
        dit.extend(f.finir());
        assert_eq!(dit.last().map(String::as_str), Some("Le reste est à l'écran."));
        assert_eq!(pages[0], dit[..dit.len() - 1].join(" "));
        // Chaque phrase une fois, le tableau à l'écran seulement, puis le détail, dans sa page.
        let tout = pages.join(" ");
        for p in &phrases {
            assert_eq!(tout.matches(p.as_str()).count(), 1, "{p}");
        }
        assert!(!tout.contains('|'));
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[2], "Le détail : une note écrite.");
        assert_eq!(pages_pour_la_voix("Court."), ["Court."]);
        assert!(pages_pour_la_voix("| a | b |\n|---|---|\n| 1 | 2 |").is_empty());
    }

    #[test]
    fn la_suite_repete_et_la_sourdine_se_reconnaissent() {
        assert!(demande_la_suite("La suite."));
        assert!(demande_la_suite("Lis-moi la suite, s'il te plaît"));
        assert!(!demande_la_suite("Continue"), "« continue » : pour Claude");
        assert!(!demande_la_suite("La suite du projet de toiture ?"));
        assert!(demande_de_repeter("Répète !"));
        assert!(demande_de_repeter("Pardon ?"));
        assert!(!demande_de_repeter("Répète-moi les échéances de la semaine"));
        assert_eq!(reglage_de_la_sourdine("Sourdine"), Some(true));
        assert_eq!(reglage_de_la_sourdine("Mode silencieux"), Some(true));
        assert_eq!(reglage_de_la_sourdine("Parle."), Some(false));
        assert_eq!(reglage_de_la_sourdine("Tu peux parler, stp"), Some(false));
        assert_eq!(reglage_de_la_sourdine("Parle-moi de la météo"), None);
        assert!(est_un_arret("Silence !"), "« silence » arrête ce qui se dit, sans la sourdine");
        assert_eq!(reglage_de_la_sourdine("Silence !"), None);
    }

    #[test]
    fn les_pannes_se_disent() {
        let (ecrit, dit) = phrase_de_panne("authentication_failed", None, false).unwrap();
        assert!(ecrit.contains("/login") && dit.ends_with("Le détail est à l'écran."));
        assert_eq!(
            phrase_de_panne("rate_limit", Some("15 h 20"), true).unwrap().1,
            "Ma limite d'utilisation de Claude est atteinte jusqu'à 15 h 20. Tu peux choisir Sonnet dans Réglages, Cerveau."
        );
        assert_eq!(
            phrase_de_panne("rate_limit", None, false).unwrap().1,
            "Ma limite d'utilisation de Claude est atteinte."
        );
        assert!(phrase_de_panne("reseau", None, false).unwrap().0.contains("connexion Internet"));
        assert_eq!(phrase_de_panne("unknown", None, false), None);
        assert_eq!(phrase_de_panne("", None, false), None);
    }

    #[test]
    fn le_point_se_reconnait() {
        assert!(demande_le_point("Le point."));
        assert!(demande_le_point("Fais-moi le point, s'il te plaît"));
        assert!(demande_le_point("Eve, fais-moi le point"), "tapé avec le nom d'Eve");
        assert!(demande_la_suite("Ok Eve, la suite"));
        assert!(!demande_le_point("Le point sur le chantier de la rue Principale"), "un autre point : pour Claude");
    }

    #[test]
    fn la_question_du_cout_se_reconnait() {
        assert!(demande_le_cout("Combien tu m'as coûté aujourd'hui ?"));
        assert!(demande_le_cout("combien tu coûtes ?"));
        assert!(demande_le_cout("Combien Claude a coûté aujourd'hui"));
        assert!(!demande_le_cout("Combien m'a coûté la toiture ?"), "le coût d'autre chose : une demande");
        assert!(!demande_le_cout("Combien ça coûte, un abonnement Claude Max ?"));
    }

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
