//! La diction : le texte tel qu'il se dit, juste avant la voix.
//!
//! Piper lit le texte par eSpeak, qui dit bien la plupart des nombres français, mais pas tout. Relevé phonème par
//! phonème le 2026-10-26, sur ce qu'Eve dit vraiment : « 14 h 30 » devient « quatorze ache trente » ; « -3 °C » perd
//! son signe ; « M. Tremblay » se dit « èm » et coupe la phrase ; « 12,99 $ » se dit « douze virgule
//! quatre-vingt-dix-neuf dollar » ; « l'ERP » se lit comme un mot, « 2e » comme « deux e » ; et « Outlook », « Gmail »,
//! « REER », « d'aujourd'hui », jusqu'au nom d'Eve, se lisent avec des sons anglais que la voix française n'a jamais
//! appris. Et un chiffre n'a pas de voyelle : la bouche du portrait, qui suit les syllabes du texte, resterait immobile
//! sur « 14 h 30 ». Ici, le texte devient des mots, comme on les dit au Québec, sans rien changer au sens : les nombres
//! et les montants, les heures, les dates, les unités, les ordinaux, les numéros de téléphone, les adresses, les
//! abréviations, les sigles qui s'épellent, et les mots anglais du monde d'Eve écrits comme ils se prononcent. Le reste
//! passe tel quel, la ponctuation et les blancs aussi : Piper en tire ses pauses.
//!
//! Relevé le 2026-10-27 sur les 1 272 phrases que l'Assistante a vraiment dites, phonème par phonème : les homographes
//! qu'eSpeak lit d'une seule façon (« en fait » sans son t, « en plus » sans son s, « vas-y » : « vas i grec »), et la
//! mélodie. Piper ne connaît que la ponctuation pour respirer : une incise entre parenthèses ou entre deux tirets
//! simples se lit d'un trait, sans pause (eSpeak n'y voit pas de fin de proposition) ; elle passe entre virgules.

use std::collections::BTreeMap;

/// Le texte tel qu'il se dit : ce que reçoivent la voix (Piper, ou une voix de Windows) et la bouche du portrait.
pub fn tel_qu_il_se_dit(texte: &str) -> String {
    // Les espaces insécables (celles des milliers, et devant « $ », « % », « : ») sont des espaces comme les autres.
    let c = texte.chars().map(|x| if matches!(x, '\u{a0}' | '\u{202f}' | '\u{2009}') { ' ' } else { x }).collect();
    let mut d = Diction { c, i: 0, sortie: String::with_capacity(texte.len() * 2), apres_un_nombre: false };
    while d.i < d.c.len() {
        if !(d.nombre() || d.mot() || d.signe()) {
            let x = d.c[d.i];
            d.sortie.push(x);
            d.apres_un_nombre &= x.is_whitespace();
            d.i += 1;
        }
    }
    d.sortie
}

/// Les prononciations de la personne (`prononciations` de `reglages.json`, que posent `eve --prononcer` et Claude
/// quand elle dit qu'un mot sonne mal) : un mot, ou quelques mots, et ce que la voix lit à sa place (« Granby » :
/// « Granbi »). Elles passent avant la diction ([`tel_qu_il_se_dit`]) : ses mots à elle l'emportent sur le lexique.
/// En mots entiers, sans égard à la casse ni à l'apostrophe (droite ou courbe), les plus longs d'abord ; un mot écrit
/// avec une majuscule la garde.
pub fn avec_les_prononciations(texte: &str, prononciations: &BTreeMap<String, String>) -> String {
    let bas = |x: char| if x == '’' { '\'' } else { x.to_lowercase().next().unwrap_or(x) };
    let mut cles: Vec<(Vec<char>, &str)> = prononciations
        .iter()
        .filter(|(mot, dit)| !mot.trim().is_empty() && !dit.trim().is_empty())
        .map(|(mot, dit)| (mot.trim().chars().map(bas).collect(), dit.trim()))
        .collect();
    if cles.is_empty() {
        return texte.to_string();
    }
    cles.sort_by_key(|(c, _)| std::cmp::Reverse(c.len()));
    let c: Vec<char> = texte.chars().collect();
    let minuscules: Vec<char> = c.iter().copied().map(bas).collect();
    let mut sortie = String::with_capacity(texte.len());
    let mut i = 0;
    while i < c.len() {
        let au_debut_d_un_mot = i == 0 || !c[i - 1].is_alphanumeric();
        let trouve = cles.iter().find(|(cle, _)| {
            au_debut_d_un_mot
                && minuscules[i..].starts_with(cle)
                && c.get(i + cle.len()).is_none_or(|x| !x.is_alphanumeric())
        });
        match trouve {
            Some((cle, dit)) => {
                // La première lettre a la casse de celle du texte : « Granby » donne « Granbi », « granby » « granbi ».
                let mut d = dit.chars();
                if let Some(premiere) = d.next() {
                    if c[i].is_uppercase() {
                        sortie.extend(premiere.to_uppercase());
                    } else {
                        sortie.extend(premiere.to_lowercase());
                    }
                    sortie.extend(d);
                }
                i += cle.len();
            }
            None => {
                sortie.push(c[i]);
                i += 1;
            }
        }
    }
    sortie
}

const UNITES: [&str; 17] = [
    "zéro", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf", "dix", "onze", "douze", "treize",
    "quatorze", "quinze", "seize",
];
const DIZAINES: [&str; 7] = ["", "dix", "vingt", "trente", "quarante", "cinquante", "soixante"];
const MOIS: [&str; 12] = [
    "janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre",
    "décembre",
];

/// Un nombre de 0 à 99 ; `pluriel` : « quatre-vingts » prend son s (au bout du nombre).
fn moins_de_cent(n: u64, pluriel: bool) -> String {
    match n {
        0..=16 => UNITES[n as usize].to_string(),
        17..=19 => format!("dix-{}", UNITES[n as usize - 10]),
        20..=69 => {
            let d = DIZAINES[n as usize / 10];
            match n % 10 {
                0 => d.to_string(),
                1 => format!("{d} et un"),
                u => format!("{d}-{}", UNITES[u as usize]),
            }
        }
        71 => "soixante et onze".to_string(),
        70..=79 => format!("soixante-{}", moins_de_cent(n - 60, false)),
        80 if pluriel => "quatre-vingts".to_string(),
        80 => "quatre-vingt".to_string(),
        _ => format!("quatre-vingt-{}", moins_de_cent(n - 80, false)),
    }
}

/// Un nombre de 1 à 999 ; `pluriel` : « cents » et « quatre-vingts » prennent leur s (au bout du nombre, ou devant
/// million et milliard ; jamais devant mille).
fn moins_de_mille(n: u64, pluriel: bool) -> String {
    let (c, r) = (n / 100, n % 100);
    let centaines = match c {
        0 => return moins_de_cent(r, pluriel),
        1 => "cent".to_string(),
        _ => format!("{} cent{}", UNITES[c as usize], if r == 0 && pluriel { "s" } else { "" }),
    };
    if r == 0 { centaines } else { format!("{centaines} {}", moins_de_cent(r, pluriel)) }
}

/// Un entier en lettres, jusqu'aux centaines de milliards, au masculin ou au féminin (« vingt et une heures »).
pub fn en_lettres(n: u64, feminin: bool) -> String {
    if n == 0 {
        return "zéro".into();
    }
    let mut mots = Vec::new();
    for (groupe, nom) in [(n / 1_000_000_000 % 1000, "milliard"), (n / 1_000_000 % 1000, "million")] {
        if groupe > 0 {
            mots.push(format!("{} {nom}{}", moins_de_mille(groupe, true), if groupe > 1 { "s" } else { "" }));
        }
    }
    match n / 1000 % 1000 {
        0 => {}
        1 => mots.push("mille".into()),
        m => mots.push(format!("{} mille", moins_de_mille(m, false))),
    }
    let reste = n % 1000;
    if reste > 0 {
        let mut r = moins_de_mille(reste, true);
        if feminin && (r == "un" || r.ends_with(" un") || r.ends_with("-un")) {
            r.push('e');
        }
        mots.push(r);
    }
    mots.join(" ")
}

/// Un ordinal en lettres : « premier », « première », « deuxième », « vingt et unième », « centième ».
pub fn ordinal(n: u64, feminin: bool) -> String {
    if n == 1 {
        return if feminin { "première" } else { "premier" }.into();
    }
    let mut m = en_lettres(n, false);
    if m.ends_with("cents") || m.ends_with("vingts") {
        m.pop();
    }
    if let Some(r) = m.strip_suffix("cinq") {
        format!("{r}cinquième")
    } else if let Some(r) = m.strip_suffix("neuf") {
        format!("{r}neuvième")
    } else {
        if m.ends_with('e') {
            m.pop();
        }
        m + "ième"
    }
}

/// Des chiffres un à un : un numéro, un code.
fn chiffre_a_chiffre(chiffres: &str) -> String {
    chiffres.chars().filter_map(|c| c.to_digit(10)).map(|d| UNITES[d as usize]).collect::<Vec<_>>().join(" ")
}

/// Des chiffres qui forment un entier : en lettres ; un à un s'ils commencent par un zéro (« 05 », « 007 ») ou si
/// `long` et qu'ils sont plus de neuf (un numéro, un code plutôt qu'un nombre).
fn entier_dit(chiffres: &str, long: bool, feminin: bool) -> String {
    let code = (chiffres.len() > 1 && chiffres.starts_with('0')) || (chiffres.len() > 9 && !long);
    match chiffres.parse::<u64>() {
        Ok(v) if !code && chiffres.len() <= 12 => en_lettres(v, feminin),
        _ => chiffre_a_chiffre(chiffres),
    }
}

/// Ce qui suit la virgule : « 5 » cinq, « 05 » zéro cinq, « 75 » soixante-quinze ; au-delà de trois chiffres, un à un.
fn decimales_en_lettres(d: &str) -> String {
    let zeros = d.chars().take_while(|&c| c == '0').count();
    let reste = &d[zeros..];
    let mut mots = vec!["zéro".to_string(); zeros];
    if reste.len() > 3 {
        mots.push(chiffre_a_chiffre(reste));
    } else if !reste.is_empty() {
        mots.push(en_lettres(reste.parse().unwrap_or(0), false));
    }
    mots.join(" ")
}

/// Une heure ou une durée : « quatorze heures trente », « une heure », « deux heures trente minutes ».
fn heure_dite(h: u64, m: Option<u64>, minutes: bool) -> String {
    let mut s = format!("{} heure{}", en_lettres(h, true), if h > 1 { "s" } else { "" });
    if let Some(m) = m.filter(|&m| m > 0) {
        s.push(' ');
        s.push_str(&en_lettres(m, true));
        if minutes {
            s.push_str(if m > 1 { " minutes" } else { " minute" });
        }
    }
    s
}

/// Une date : « vingt-six octobre deux mille vingt-six », « premier novembre » (le jour et le mois sont valides).
fn date_dite(jour: u64, mois: u64, annee: Option<u64>) -> String {
    let j = if jour == 1 { "premier".to_string() } else { en_lettres(jour, false) };
    let a = annee.map(|a| format!(" {}", en_lettres(a, false))).unwrap_or_default();
    format!("{j} {}{a}", MOIS[mois as usize - 1])
}

fn date_valide(jour: u64, mois: u64) -> bool {
    (1..=12).contains(&mois) && (1..=31).contains(&jour)
}

/// La valeur d'un nombre romain (« XXI »), s'il est écrit comme il se doit (« IIII » n'en est pas un).
fn valeur_romaine(s: &str) -> Option<u64> {
    let valeur = |c: char| match c {
        'I' => Some(1),
        'V' => Some(5),
        'X' => Some(10),
        'L' => Some(50),
        'C' => Some(100),
        'D' => Some(500),
        'M' => Some(1000),
        _ => None,
    };
    let v: Vec<i64> = s.chars().map(valeur).collect::<Option<_>>()?;
    let total: i64 =
        v.iter().enumerate().map(|(k, &c)| if v.get(k + 1).is_some_and(|&n| n > c) { -c } else { c }).sum();
    let total = u64::try_from(total).ok().filter(|&t| t > 0)?;
    (en_romain(total) == s).then_some(total)
}

fn en_romain(mut n: u64) -> String {
    const ROMAINS: [(u64, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for (v, r) in ROMAINS {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// Un nombre lu dans le texte.
struct Nombre {
    /// Ses chiffres, sans les espaces des milliers.
    chiffres: String,
    /// Ses milliers étaient séparés (« 1 250 ») : c'est bien un nombre, même long.
    groupe: bool,
    /// Ce qui suit la virgule (ou un seul point : « 937.05 »).
    decimales: Option<String>,
    /// Le séparateur des décimales était une virgule.
    virgule: bool,
    /// Après d'autres points : une version, une adresse IP (« 2.1.3 »), les parties après la première.
    points: Vec<String>,
    /// Là où il finit.
    fin: usize,
}

impl Nombre {
    fn entier(&self) -> bool {
        self.decimales.is_none()
    }

    /// Sa partie entière, si elle tient dans un nombre qui se dit.
    fn valeur(&self) -> Option<u64> {
        (self.chiffres.len() <= 12).then(|| self.chiffres.parse().ok()).flatten()
    }

    /// Deux ou plus : le nom qui le suit est au pluriel (« 1,5 kilomètre », « 2 kilomètres »).
    fn pluriel(&self) -> bool {
        self.valeur().is_none_or(|v| v >= 2)
    }

    /// Tout le nombre en mots : « un virgule cinq », « deux point un point trois » ; `feminin` s'il est entier.
    fn en_mots(&self, feminin: bool) -> String {
        let entier = entier_dit(&self.chiffres, self.groupe, feminin && self.entier());
        match &self.decimales {
            None => entier,
            Some(d) if self.points.is_empty() => {
                format!("{entier} {} {}", if self.virgule { "virgule" } else { "point" }, decimales_en_lettres(d))
            }
            // Une version, une adresse IP : chaque partie, après son point.
            Some(d) => std::iter::once(d)
                .chain(&self.points)
                .fold(entier, |s, p| format!("{s} point {}", entier_dit(p, false, false))),
        }
    }
}

/// Une monnaie : son nom, et celui de son centième — le « cent » du Canada se dit [sɛnt], comme eSpeak le dit du
/// « ¢ » : écrit « cènte » (« cents » se lirait [sɑ̃], comme « deux cents »), qui se dit pareil au pluriel.
#[derive(Clone, Copy, PartialEq)]
enum Devise {
    Dollar,
    Euro,
}

/// Un montant : « deux cent cinquante dollars », « douze dollars quatre-vingt-dix-neuf », « quatre cènte » (0,04 $),
/// « cinq millions de dollars ». Au-delà de deux décimales (« 0,004 $ »), tel quel : « zéro virgule zéro zéro quatre
/// dollar ».
fn montant(n: &Nombre, devise: Devise) -> String {
    let (nom, centieme) = match devise {
        Devise::Dollar => ("dollar", "cènte"),
        Devise::Euro => ("euro", "centime"),
    };
    let s = |pluriel: bool| if pluriel { "s" } else { "" };
    match &n.decimales {
        Some(d) if d.len() <= 2 && n.points.is_empty() => {
            let c: u64 = if d.len() == 1 { d.parse::<u64>().unwrap_or(0) * 10 } else { d.parse().unwrap_or(0) };
            let unites = entier_dit(&n.chiffres, n.groupe, false);
            match (n.valeur() == Some(0), c) {
                (true, 0) => format!("zéro {nom}"),
                (true, c) => format!("{} {centieme}{}", en_lettres(c, false), s(c > 1 && devise == Devise::Euro)),
                (false, 0) => format!("{unites} {nom}{}", s(n.pluriel())),
                (false, c) => format!("{unites} {nom}{} {}", s(n.pluriel()), en_lettres(c, false)),
            }
        }
        None => {
            // « un million de dollars » : million et milliard sont des noms.
            let de = n.valeur().is_some_and(|v| v >= 1_000_000 && v % 1_000_000 == 0);
            format!("{} {}{nom}{}", n.en_mots(false), if de { "de " } else { "" }, s(n.pluriel()))
        }
        _ => format!("{} {nom}{}", n.en_mots(false), s(n.pluriel())),
    }
}

/// Les unités après un nombre, qu'eSpeak lit mal (« 70 km/h » : « kilomètres ache », « 2 L » : « èl ») ou que la
/// bouche veut en lettres : leur forme écrite, leur nom au singulier et au pluriel, et s'il est féminin (« une
/// minute »). Les plus longues d'abord. « h » et « s » ne valent qu'après un nombre : « 1,5 h », « 30 s ».
const UNITES_DE_MESURE: [(&str, &str, &str, bool); 53] = [
    ("km/h", "kilomètre à l'heure", "kilomètres à l'heure", false),
    ("Mbit/s", "mégabit par seconde", "mégabits par seconde", false),
    ("Mbps", "mégabit par seconde", "mégabits par seconde", false),
    ("Mo/s", "mégaoctet par seconde", "mégaoctets par seconde", false),
    ("m/s", "mètre par seconde", "mètres par seconde", false),
    ("kWh", "kilowattheure", "kilowattheures", false),
    ("km²", "kilomètre carré", "kilomètres carrés", false),
    ("pi²", "pied carré", "pieds carrés", false),
    ("pi2", "pied carré", "pieds carrés", false),
    ("pi³", "pied cube", "pieds cubes", false),
    ("vg³", "verge cube", "verges cubes", true),
    ("m²", "mètre carré", "mètres carrés", false),
    ("m2", "mètre carré", "mètres carrés", false),
    ("m³", "mètre cube", "mètres cubes", false),
    ("m3", "mètre cube", "mètres cubes", false),
    ("GHz", "gigahertz", "gigahertz", false),
    ("MHz", "mégahertz", "mégahertz", false),
    ("kHz", "kilohertz", "kilohertz", false),
    ("Hz", "hertz", "hertz", false),
    ("min", "minute", "minutes", true),
    ("sec", "seconde", "secondes", true),
    ("ms", "milliseconde", "millisecondes", true),
    ("km", "kilomètre", "kilomètres", false),
    ("cm", "centimètre", "centimètres", false),
    ("mm", "millimètre", "millimètres", false),
    ("kg", "kilogramme", "kilogrammes", false),
    ("mg", "milligramme", "milligrammes", false),
    ("lbs", "livre", "livres", true),
    ("lb", "livre", "livres", true),
    ("oz", "once", "onces", true),
    ("ml", "millilitre", "millilitres", false),
    ("mL", "millilitre", "millilitres", false),
    ("Mo", "mégaoctet", "mégaoctets", false),
    ("Go", "gigaoctet", "gigaoctets", false),
    ("Ko", "kilooctet", "kilooctets", false),
    ("ko", "kilooctet", "kilooctets", false),
    ("To", "téraoctet", "téraoctets", false),
    ("MB", "mégaoctet", "mégaoctets", false),
    ("GB", "gigaoctet", "gigaoctets", false),
    ("KB", "kilooctet", "kilooctets", false),
    ("TB", "téraoctet", "téraoctets", false),
    ("kW", "kilowatt", "kilowatts", false),
    ("W", "watt", "watts", false),
    ("pi", "pied", "pieds", false),
    ("po", "pouce", "pouces", false),
    ("vg", "verge", "verges", true),
    ("m", "mètre", "mètres", false),
    ("g", "gramme", "grammes", false),
    ("t", "tonne", "tonnes", true),
    ("L", "litre", "litres", false),
    ("l", "litre", "litres", false),
    ("h", "heure", "heures", true),
    ("s", "seconde", "secondes", true),
];

/// Les noms féminins les plus courants dans ce que dit Eve : devant eux, « 1 » et « 21 » se disent « une », « vingt et
/// une » (eSpeak dirait « un »).
const FEMININS: [&str; 74] = [
    "heure", "minute", "seconde", "semaine", "journée", "année", "fois", "personne", "facture", "soumission", "tâche",
    "page", "étape", "ligne", "chose", "demande", "réponse", "question", "idée", "photo", "image", "maison", "voiture",
    "pièce", "porte", "fenêtre", "chambre", "salle", "place", "commande", "livraison", "réunion", "rencontre",
    "visite", "note", "lettre", "adresse", "entreprise", "équipe", "version", "erreur", "alerte", "partie", "option",
    "solution", "offre", "estimation", "dépense", "taxe", "date", "nuit", "soirée", "matinée", "planche", "feuille",
    "couche", "école", "fête", "sortie", "classe", "carte", "clé", "vidéo", "application", "tranche", "remise",
    "échéance", "rue", "ville", "région", "unité", "boîte", "bouteille", "tonne",
];

/// Les sigles qu'eSpeak lit comme un mot (« l'ERP » : [lɛʁp], « la SAQ » : [sak], « un CPE » : [kp]) alors qu'ils
/// s'épellent ; épelés, leurs lettres séparées d'une espace, ils se disent comme ceux qu'il épelle de lui-même
/// (« TPS », « CLSC »). Avec des traits d'union, « l'I-A » glissait en [lja] ; « l'I A » se dit [li a]. Ceux qui se
/// disent comme un mot au Québec restent des mots (« la CAQ », « le CÉGEP », « l'UQAM », « le NIP », « le REM »).
const SIGLES_EPELES: [&str; 31] = [
    "ERP", "IA", "SAAQ", "RAMQ", "ARC", "CA", "ACQ", "APCHQ", "GPU", "SAQ", "PIB", "HEC", "TI", "CNESST", "REEE",
    "CMEQ", "OIQ", "OAQ", "SEAO", "AEC", "ETS", "VUS", "CPE", "AE", "RQAP", "UE", "NEQ", "ASP", "CPTAQ", "TDAH", "IGA",
];

/// Les mots qu'eSpeak lit avec des sons anglais (ou à côté), écrits comme on les dit au Québec ; chaque graphie,
/// vérifiée dans eSpeak : rien que des phonèmes français. Sans égard à la casse, sauf ceux qui commencent par une
/// majuscule (« python », le serpent, se dit en français). Un pluriel en s (« emails ») prend la même graphie. Les
/// villes du Québec au nom anglais ou autochtone qu'eSpeak lit à l'anglaise (relevé le 2026-10-26 sur 120 villes :
/// « Granby » y devient « gɹanbi », avec un r anglais et sans nasale), comme les y disent les francophones. Et un mot
/// bien français qu'eSpeak prend pour de l'anglais (relevé le 2026-10-27 : « menthe », [mɛnð]).
const LEXIQUE: [(&str, &str); 116] = [
    ("eve", "Ève"),
    ("aujourd'hui", "aujourdhui"),
    ("menthe", "mante"),
    ("Stripe", "Straïpe"),
    ("Render", "Rènndeur"),
    ("Jerry", "Djèri"),
    ("reer", "Èrééèr"),
    ("celi", "Céli"),
    ("celiapp", "Céliapp"),
    ("json", "Djéssonne"),
    ("haiku", "Haïku"),
    ("outlook", "Aoutlouk"),
    ("gmail", "Djimèle"),
    ("hotmail", "Hotmèle"),
    ("google", "Gougueule"),
    ("windows", "Ouinedoze"),
    ("teams", "Timmze"),
    ("word", "Oueurde"),
    ("onedrive", "Ouane-draïve"),
    ("powerpoint", "Paoueur-pointe"),
    ("powershell", "Paoueur-chèl"),
    ("facebook", "Fèsbouk"),
    ("youtube", "Youtoube"),
    ("linkedin", "Linnkédine"),
    ("messenger", "Messinndjeur"),
    ("skype", "Skaïpe"),
    ("dropbox", "Dropboxe"),
    ("icloud", "Aïclaoude"),
    ("bluetooth", "Bloutousse"),
    ("ikea", "Ikéa"),
    ("walmart", "Oualmarte"),
    ("mcdonald's", "Mac-Donalde"),
    ("anthropic", "Anne-tropique"),
    ("openai", "Opène-A-I"),
    ("whisper", "Ouisspeur"),
    ("piper", "Païpeur"),
    ("Rust", "Reusste"),
    ("Python", "Païtonne"),
    ("github", "Guitt-Heub"),
    ("iphone", "Aïfone"),
    ("ipad", "Aïpade"),
    ("macbook", "Mac-bouk"),
    ("quickbooks", "Kouik-bouks"),
    ("sketchup", "Sketch-eup"),
    ("firefox", "Faïeur-fox"),
    ("shopify", "Chopifaï"),
    ("wordpress", "Oueurde-presse"),
    ("halloween", "Halloouine"),
    ("email", "imèle"),
    ("e-mail", "imèle"),
    ("weekend", "ouikènde"),
    ("week-end", "ouikènde"),
    ("meeting", "mîtigne"),
    ("deadline", "dèdlaïne"),
    ("cool", "coule"),
    ("feedback", "fidbaque"),
    ("backup", "bacqueupe"),
    ("update", "eupdéte"),
    ("upgrade", "eupgréde"),
    ("download", "daoune-lôde"),
    ("upload", "eup-lôde"),
    ("login", "loguine"),
    ("password", "passeouorde"),
    ("cloud", "claoude"),
    ("design", "dizaïne"),
    ("designer", "dizaïneur"),
    ("marketing", "markétigne"),
    ("coach", "côtche"),
    ("coaching", "côtchigne"),
    ("startup", "starteupe"),
    ("streaming", "strîmigne"),
    ("dashboard", "dache-borde"),
    ("leader", "lideur"),
    ("plugin", "pleuguine"),
    ("lunch", "leunche"),
    ("business", "bizznèss"),
    ("workflow", "oueurkflô"),
    ("whatsapp", "Wattsape"),
    ("uber", "Oubeur"),
    ("hortons", "Hortonne"),
    ("tracy", "Traci"),
    ("sherbrooke", "Cherbrouk"),
    ("hyacinthe", "Iacinte"),
    ("windsor", "Ouinnzor"),
    ("hudson", "Heudsonne"),
    ("beaconsfield", "Bîconsfîld"),
    ("valleyfield", "Valléfîld"),
    ("thetford", "Tèttforde"),
    ("granby", "Granbi"),
    ("waterloo", "Ouatèrlou"),
    ("waterville", "Ouatèrville"),
    ("coaticook", "Coaticouk"),
    ("knowlton", "Noltonne"),
    ("sutton", "Seuttonne"),
    ("dunham", "Donnam"),
    ("marieville", "Mariville"),
    ("frelighsburg", "Frélisbourg"),
    ("sweetsburg", "Souitsbourg"),
    ("kingsbury", "Kinngzbéri"),
    ("kingsey", "Kinngsi"),
    ("warwick", "Ouarouik"),
    ("ormstown", "Ormstaoune"),
    ("howick", "Haouik"),
    ("rawdon", "Rodonne"),
    ("wakefield", "Ouèkfilde"),
    ("shawville", "Chôville"),
    ("cookshire", "Coukchaïre"),
    ("weedon", "Ouidonne"),
    ("Bury", "Béri"),
    ("Pike", "Païk"),
    ("kahnawake", "Kanawaké"),
    ("kanesatake", "Kanéssataké"),
    ("wendake", "Ouèndaké"),
    ("calgary", "Calgari"),
    ("winnipeg", "Ouinnipègue"),
    ("Roy", "Roi"),
];

/// Des expressions de plusieurs mots qu'eSpeak lit en anglais.
const EXPRESSIONS: [(&str, &str); 3] =
    [("Black Friday", "Blac Fraïdé"), ("Boxing Day", "Boxigne Dé"), ("New York", "Niou-Yorke")];

/// Ce qui doit suivre une abréviation pour qu'elle en soit une.
#[derive(Clone, Copy)]
enum Suite {
    /// Rien de particulier (si elle finit sans point : la fin du mot).
    Tout,
    /// Un nom : « M. Tremblay », « Me Lavoie » (« Me voici » n'en est pas).
    Majuscule,
    /// Un numéro : « app. 4 ».
    Chiffre,
}

/// Les abréviations : leur forme écrite, ce qui se dit, et ce qui doit les suivre. Les plus longues d'abord. Un point
/// d'abréviation que Piper prendrait pour une fin de phrase (« M. » : une pause, puis « Tremblay » seul) s'en va avec
/// elle.
const ABREVIATIONS: [(&str, &str, Suite); 43] = [
    ("c.-à-d.", "c'est-à-dire", Suite::Tout),
    ("c.-à-d", "c'est-à-dire", Suite::Tout),
    ("c-à-d", "c'est-à-dire", Suite::Tout),
    ("p. ex.", "par exemple", Suite::Tout),
    ("p.ex.", "par exemple", Suite::Tout),
    ("P. ex.", "Par exemple", Suite::Tout),
    ("Ex. :", "Par exemple :", Suite::Tout),
    ("ex. :", "par exemple :", Suite::Tout),
    ("N.B.", "nota bene", Suite::Tout),
    ("P.-S.", "post-scriptum", Suite::Tout),
    ("P.S.", "post-scriptum", Suite::Tout),
    ("s.v.p.", "s'il vous plaît", Suite::Tout),
    ("svp", "s'il vous plaît", Suite::Tout),
    ("SVP", "s'il vous plaît", Suite::Tout),
    ("stp", "s'il te plaît", Suite::Tout),
    ("STP", "s'il te plaît", Suite::Tout),
    ("MM. ", "messieurs ", Suite::Majuscule),
    ("M. ", "monsieur ", Suite::Majuscule),
    ("Me ", "maître ", Suite::Majuscule),
    ("n° ", "numéro ", Suite::Chiffre),
    ("n°", "numéro ", Suite::Chiffre),
    ("N° ", "numéro ", Suite::Chiffre),
    ("N°", "numéro ", Suite::Chiffre),
    ("no ", "numéro ", Suite::Chiffre),
    ("No ", "numéro ", Suite::Chiffre),
    ("Mmes", "mesdames", Suite::Tout),
    ("Mme", "madame", Suite::Tout),
    ("Mlles", "mesdemoiselles", Suite::Tout),
    ("Mlle", "mademoiselle", Suite::Tout),
    ("Dre", "docteure", Suite::Tout),
    ("Dr", "docteur", Suite::Tout),
    ("boul.", "boulevard", Suite::Tout),
    ("av.", "avenue", Suite::Tout),
    ("app. ", "appartement ", Suite::Chiffre),
    ("bur. ", "bureau ", Suite::Chiffre),
    ("Tél.", "téléphone", Suite::Tout),
    ("tél.", "téléphone", Suite::Tout),
    ("env.", "environ", Suite::Tout),
    ("vs.", "versus", Suite::Tout),
    ("vs", "versus", Suite::Tout),
    ("ltée", "limitée", Suite::Tout),
    ("Ltée", "limitée", Suite::Tout),
    ("Ltd", "limitée", Suite::Tout),
];

/// Les mots qu'une apostrophe élide devant un sigle ou un mot du lexique : « l'ERP », « d'Outlook ».
const ELISIONS: [&str; 13] = ["l", "d", "j", "m", "n", "s", "t", "c", "qu", "jusqu", "lorsqu", "puisqu", "quoiqu"];

/// Les mots qui annoncent une date (« le 26/10 ») plutôt qu'une fraction ou une note (« 3/4 », « 8/10 »).
const AVANT_UNE_DATE: [&str; 15] = [
    "le", "du", "au", "dès", "depuis", "avant", "après", "ce", "lundi", "mardi", "mercredi", "jeudi", "vendredi",
    "samedi", "dimanche",
];

/// Les fins des adresses du Web, et comment elles se disent (« point C A »).
const DOMAINES: [(&str, &str); 10] = [
    ("ca", "C A"),
    ("qc", "Q C"),
    ("com", "com"),
    ("org", "org"),
    ("net", "nète"),
    ("io", "I O"),
    ("ai", "A I"),
    ("fr", "F R"),
    ("info", "info"),
    ("gouv", "gouv"),
];

/// Un mot seul, tel qu'il se dit : un sigle à épeler, un mot du lexique (après l'élision : « l'ERP », « d'Outlook »),
/// ou l'une des parties d'un mot composé (« Saint-Hyacinthe », « St-Jean ») — le « y » d'un impératif, « i » : eSpeak
/// lisait « vas-y » « vas i grec » ; sinon, tel quel.
fn mot_dit(mot: &str) -> String {
    let (prefixe, reste) = elision(mot);
    // Devant un sigle épelé, l'article en minuscule : « L'I-A » se lirait « L apostrophe I A ».
    if let Some(dit) = remplacer(reste) {
        return format!("{}{dit}", prefixe.to_lowercase());
    }
    if reste.contains('-') {
        let parties: Vec<&str> = reste.split('-').collect();
        let dites: Vec<String> = parties
            .iter()
            .enumerate()
            .map(|(n, p)| {
                let nom_suit = parties.get(n + 1).is_some_and(|s| s.starts_with(char::is_uppercase));
                match *p {
                    "St" if nom_suit => "Saint".to_string(),
                    "Ste" if nom_suit => "Sainte".to_string(),
                    "y" if n > 0 => "i".to_string(),
                    _ => remplacer(p).unwrap_or_else(|| p.to_string()),
                }
            })
            .collect();
        return format!("{prefixe}{}", dites.join("-"));
    }
    mot.to_string()
}

/// L'élision en tête d'un mot (« l' », « d' », « jusqu' »), et le reste.
fn elision(mot: &str) -> (&str, &str) {
    if let Some((p, a)) = mot.char_indices().find(|(_, c)| matches!(c, '\'' | '’'))
        && ELISIONS.contains(&mot[..p].to_lowercase().as_str())
    {
        return mot.split_at(p + a.len_utf8());
    }
    ("", mot)
}

/// Un sigle épelé (« E-R-P »), ou la graphie du lexique, la majuscule du mot gardée ; pour un pluriel en s, celle du
/// singulier.
fn remplacer(m: &str) -> Option<String> {
    if SIGLES_EPELES.contains(&m) {
        return Some(m.chars().map(String::from).collect::<Vec<_>>().join(" "));
    }
    let bas = m.to_lowercase().replace('’', "'");
    // « rythme », « rythmé », « rythmique » : eSpeak y entend l'anglais « rhythm ».
    if let Some(reste) = bas.strip_prefix("rythm") {
        let r = if m.starts_with(char::is_uppercase) { "Ritm" } else { "ritm" };
        return Some(format!("{r}{reste}"));
    }
    let trouve =
        |cle: &str| LEXIQUE.iter().find(|(e, _)| if e.starts_with(char::is_uppercase) { *e == m } else { *e == cle });
    let (graphie, s) = match trouve(&bas) {
        Some((_, g)) => (*g, ""),
        None => (trouve(bas.strip_suffix('s')?)?.1, "s"),
    };
    let mut g = graphie.chars();
    let premiere = g.next()?;
    let premiere: String = if m.starts_with(char::is_uppercase) {
        premiere.to_uppercase().collect()
    } else {
        premiere.to_lowercase().collect()
    };
    Some(format!("{premiere}{}{s}", g.as_str()))
}

struct Diction {
    c: Vec<char>,
    i: usize,
    sortie: String,
    /// Ce qui vient de se dire est un nombre (« 3 », « 9 h ») : un tiret qui suit mène au suivant (« 3-5 jours »).
    apres_un_nombre: bool,
}

impl Diction {
    fn car(&self, k: usize) -> Option<char> {
        self.c.get(k).copied()
    }

    fn chiffre(&self, k: usize) -> bool {
        self.car(k).is_some_and(|x| x.is_ascii_digit())
    }

    fn lettre(&self, k: usize) -> bool {
        self.car(k).is_some_and(char::is_alphabetic)
    }

    /// Les chiffres qui se suivent à partir de `k`.
    fn chiffres(&self, k: usize) -> usize {
        self.c.get(k..).map_or(0, |r| r.iter().take_while(|x| x.is_ascii_digit()).count())
    }

    fn valeur(&self, de: usize, n: usize) -> u64 {
        self.c[de..de + n].iter().collect::<String>().parse().unwrap_or(0)
    }

    /// Une espace en `k` est sautée.
    fn espace(&self, k: usize) -> usize {
        if self.car(k) == Some(' ') { k + 1 } else { k }
    }

    /// Le texte en `k` commence par `motif`.
    fn commence(&self, k: usize, motif: &str) -> bool {
        motif.chars().enumerate().all(|(n, m)| self.car(k + n) == Some(m))
    }

    /// La fin d'un mot : rien après, ou ni lettre, ni chiffre, ni apostrophe (« 2 l'heure » n'est pas « 2 L »).
    fn fin_de_mot(&self, k: usize) -> bool {
        self.car(k).is_none_or(|x| !x.is_alphanumeric() && x != '\'' && x != '’')
    }

    /// Des mots à dire, séparés de la lettre ou du chiffre qui les précède (« F5 » : « F cinq »).
    fn dire(&mut self, mots: &str) {
        if self.sortie.ends_with(char::is_alphanumeric) && mots.starts_with(char::is_alphanumeric) {
            self.sortie.push(' ');
        }
        self.sortie.push_str(mots);
    }

    /// Un signe devant un nombre (« -3 », « +5 ») : au début, ou après un blanc ou une ouverture — pas dans « 3-5 ».
    fn signe_permis(&self, k: usize) -> bool {
        k == 0 || self.car(k - 1).is_some_and(|p| p.is_whitespace() || "([«:;/=\"".contains(p))
    }

    /// Le mot qui précède `k`, en minuscules.
    fn mot_avant(&self, k: usize) -> String {
        let (d, f) = self.bornes_du_mot_avant(k);
        self.c[d..f].iter().collect::<String>().to_lowercase()
    }

    /// Les bornes du mot qui précède `k` (ses lettres, les blancs d'entre eux sautés) ; vide s'il n'y a pas de lettre
    /// juste avant (le début, une ponctuation, une apostrophe).
    fn bornes_du_mot_avant(&self, k: usize) -> (usize, usize) {
        let mut j = k;
        while j > 0 && self.c[j - 1].is_whitespace() {
            j -= 1;
        }
        let fin = j;
        while j > 0 && self.c[j - 1].is_alphabetic() {
            j -= 1;
        }
        (j, fin)
    }

    /// Le mot qui suit `k`, en minuscules.
    fn mot_apres(&self, k: usize) -> String {
        let mut j = k;
        while self.car(j).is_some_and(char::is_whitespace) {
            j += 1;
        }
        let debut = j;
        while self.lettre(j) {
            j += 1;
        }
        self.c[debut..j].iter().collect::<String>().to_lowercase()
    }

    // ---- Les nombres ---------------------------------------------------------------------------------------------

    /// Un nombre et ce qui le suit (une heure, une date, une unité, un montant), avec le signe qui le précède.
    fn nombre(&mut self) -> bool {
        let k = self.i;
        let x = self.c[k];
        if matches!(x, '0'..='9' | '(' | '+')
            && let Some((fin, dit)) = self.telephone(k)
        {
            self.dire(&dit);
            self.i = fin;
            self.apres_un_nombre = true;
            return true;
        }
        let apres_espace = self.espace(k + 1);
        let (prefixe, debut, dollar) = match x {
            '-' | '−' | '–' if self.chiffre(k + 1) && self.signe_permis(k) && !self.apres_un_nombre => {
                ("moins ", k + 1, false)
            }
            // « plusse » : eSpeak lit « plus » [ply], comme dans « plus de lait ».
            '+' if self.chiffre(k + 1) && self.signe_permis(k) => ("plusse ", k + 1, false),
            '~' | '≈' if self.chiffre(apres_espace) => ("environ ", apres_espace, false),
            '#' if self.chiffre(k + 1) => ("numéro ", k + 1, false),
            '×' if self.chiffre(apres_espace) => ("fois ", apres_espace, false),
            '$' if self.chiffre(apres_espace) => ("", apres_espace, true),
            '0'..='9' => ("", k, false),
            _ => return false,
        };
        let Some(n) = self.lire_nombre(debut) else { return false };
        let (mut fin, mut dit) = if dollar { (n.fin, montant(&n, Devise::Dollar)) } else { self.apres(&n, debut) };
        // Des dimensions : « 2 x 4 », « 2x4 », « 12 × 14 » (le nombre qui suit se lit au tour suivant).
        let j = self.espace(fin);
        if matches!(self.car(j), Some('x' | 'X' | '×')) && self.chiffre(self.espace(j + 1)) {
            dit.push_str(" par ");
            fin = self.espace(j + 1);
        }
        self.dire(prefixe);
        self.dire(&dit);
        if self.lettre(fin) {
            self.sortie.push(' ');
        }
        self.i = fin;
        self.apres_un_nombre = true;
        true
    }

    /// Les chiffres d'un nombre en `k` : ses milliers séparés d'une espace (« 1 250 », le premier groupe d'un à trois
    /// chiffres), ses décimales après une virgule, ou ses parties après des points (« 937.05 », « 2.1.3 »).
    fn lire_nombre(&self, k: usize) -> Option<Nombre> {
        let n = self.chiffres(k);
        if n == 0 {
            return None;
        }
        let mut chiffres: String = self.c[k..k + n].iter().collect();
        let mut j = k + n;
        let mut groupe = false;
        if n <= 3 {
            while self.car(j) == Some(' ') && self.chiffres(j + 1) == 3 {
                chiffres.extend(&self.c[j + 1..j + 4]);
                j += 4;
                groupe = true;
            }
        }
        let (mut decimales, mut virgule, mut points) = (None, false, Vec::new());
        if self.car(j) == Some(',') && self.chiffre(j + 1) {
            let d = self.chiffres(j + 1);
            decimales = Some(self.c[j + 1..j + 1 + d].iter().collect());
            virgule = true;
            j += 1 + d;
        } else {
            while self.car(j) == Some('.') && self.chiffre(j + 1) {
                let d = self.chiffres(j + 1);
                points.push(self.c[j + 1..j + 1 + d].iter().collect::<String>());
                j += 1 + d;
            }
            if !points.is_empty() {
                decimales = Some(points.remove(0));
            }
        }
        Some(Nombre { chiffres, groupe, decimales, virgule, points, fin: j })
    }

    /// Ce que dit un nombre selon ce qui le suit : une date, une fraction, une heure, un ordinal, une unité ; sinon,
    /// lui seul (au féminin devant un nom féminin : « 1 fois », « 21 personnes »).
    fn apres(&self, n: &Nombre, debut: usize) -> (usize, String) {
        if n.entier()
            && let Some(v) = n.valeur()
            && let Some(r) = self
                .date_iso(n, v)
                .or_else(|| self.barre(v, n.fin, debut))
                .or_else(|| self.heure(n, v))
                .or_else(|| self.ordinal_colle(v, n.fin))
        {
            return r;
        }
        if let Some(r) = self.unite(n) {
            return r;
        }
        let suivant = self.mot_apres(n.fin);
        let feminin = FEMININS.iter().any(|f| suivant == *f || suivant.strip_suffix('s') == Some(f));
        (n.fin, n.en_mots(feminin))
    }

    /// « 2026-10-26 » : « vingt-six octobre deux mille vingt-six » ; « 2026-10 » : « octobre deux mille vingt-six ».
    fn date_iso(&self, n: &Nombre, an: u64) -> Option<(usize, String)> {
        let j = n.fin;
        if n.chiffres.len() != 4 || n.groupe || self.car(j) != Some('-') || self.chiffres(j + 1) != 2 {
            return None;
        }
        let mois = self.valeur(j + 1, 2);
        if self.car(j + 3) == Some('-') && self.chiffres(j + 4) == 2 {
            let jour = self.valeur(j + 4, 2);
            return date_valide(jour, mois).then(|| (j + 6, date_dite(jour, mois, Some(an))));
        }
        let seul = !self.chiffre(j + 3) && self.car(j + 3) != Some('-');
        (seul && (1..=12).contains(&mois) && (1900..=2100).contains(&an))
            .then(|| (j + 3, format!("{} {}", MOIS[mois as usize - 1], en_lettres(an, false))))
    }

    /// Un nombre suivi d'une barre : une date (« 26/10/2026 », « le 26/10 »), une fraction (« 3/4 ») ou une note
    /// (« 8/10 », « 24/7 »).
    fn barre(&self, a: u64, j: usize, debut: usize) -> Option<(usize, String)> {
        let nb = self.chiffres(j + 1);
        if self.car(j) != Some('/') || nb == 0 || nb > 4 {
            return None;
        }
        let b = self.valeur(j + 1, nb);
        let fin = j + 1 + nb;
        if self.car(fin) == Some('/') && matches!(self.chiffres(fin + 1), 2 | 4) {
            let ny = self.chiffres(fin + 1);
            let y = self.valeur(fin + 1, ny);
            let annee = if ny == 2 { 2000 + y } else { y };
            return date_valide(a, b).then(|| (fin + 1 + ny, date_dite(a, b, Some(annee))));
        }
        if date_valide(a, b) && AVANT_UNE_DATE.contains(&self.mot_avant(debut).as_str()) {
            return Some((fin, date_dite(a, b, None)));
        }
        let dit = match (a, b) {
            (1, 2) => "un demi".to_string(),
            (1, 3) => "un tiers".to_string(),
            (2, 3) => "deux tiers".to_string(),
            (1, 4) => "un quart".to_string(),
            (3, 4) => "trois quarts".to_string(),
            _ => format!("{} sur {}", en_lettres(a, false), en_lettres(b, false)),
        };
        Some((fin, dit))
    }

    /// Une heure ou une durée : « 14 h 30 », « 14h30 », « 9 h », « 2 h 30 min », « 14:30 ».
    fn heure(&self, n: &Nombre, h: u64) -> Option<(usize, String)> {
        if n.chiffres.len() > 3 || n.groupe {
            return None;
        }
        let k = self.espace(n.fin);
        match self.car(k) {
            Some('h' | 'H') if !self.lettre(k + 1) => {
                let mut fin = k + 1;
                let m0 = self.espace(fin);
                let nm = self.chiffres(m0);
                let (mut minutes, mut mot) = (None, false);
                let decimal = self.car(m0 + nm) == Some(',') && self.chiffre(m0 + nm + 1);
                if (1..=2).contains(&nm) && !decimal && self.valeur(m0, nm) < 60 {
                    minutes = Some(self.valeur(m0, nm));
                    fin = m0 + nm;
                    let u = self.espace(fin);
                    if self.commence(u, "min") && self.fin_de_mot(u + 3) {
                        mot = true;
                        fin = u + 3;
                    }
                }
                Some((fin, heure_dite(h, minutes, mot)))
            }
            Some(':') if k == n.fin && h <= 24 && self.chiffres(k + 1) == 2 => {
                let m = self.valeur(k + 1, 2);
                let secondes = self.car(k + 3) == Some(':') && self.chiffre(k + 4);
                (m < 60 && !secondes).then(|| (k + 3, heure_dite(h, Some(m), false)))
            }
            _ => None,
        }
    }

    /// Un ordinal collé à son nombre : « 1er », « 1re », « 2e », « 2ème », « 21es », « 2nd ».
    fn ordinal_colle(&self, v: u64, j: usize) -> Option<(usize, String)> {
        const SUFFIXES: [&str; 20] = [
            "ièmes", "ième", "ieme", "èmes", "ème", "emes", "eme", "ères", "ère", "ers", "er", "res", "re", "es", "e",
            "è", "ndes", "nde", "nds", "nd",
        ];
        let s = SUFFIXES.iter().find(|s| self.commence(j, s) && self.fin_de_mot(j + s.chars().count()))?;
        let pluriel = if s.ends_with('s') { "s" } else { "" };
        let dit = match *s {
            "er" | "ers" if v == 1 => format!("premier{pluriel}"),
            "re" | "res" | "ère" | "ères" if v == 1 => format!("première{pluriel}"),
            "nd" | "nds" if v == 2 => format!("second{pluriel}"),
            "nde" | "ndes" if v == 2 => format!("seconde{pluriel}"),
            "er" | "ers" | "re" | "res" | "ère" | "ères" | "nd" | "nds" | "nde" | "ndes" => return None,
            _ => format!("{}{pluriel}", ordinal(v, false)),
        };
        Some((j + s.chars().count(), dit))
    }

    /// Une unité, une monnaie, un pourcentage ou des degrés après le nombre (une espace au plus entre eux).
    fn unite(&self, n: &Nombre) -> Option<(usize, String)> {
        let k = self.espace(n.fin);
        let s = |pluriel: bool| if pluriel { "s" } else { "" };
        // Les millions et les milliards de dollars : « 1,5 M$ », « 2 G $ », « 3 millions $ ».
        for (motifs, nom) in [(["M$", "M $"], "million"), (["G$", "G $"], "milliard")] {
            if let Some(m) = motifs.iter().find(|m| self.commence(k, m)) {
                let dit = format!("{} {nom}{} de dollars", n.en_mots(false), s(n.pluriel()));
                return Some((k + m.chars().count(), dit));
            }
        }
        for nom in ["millions", "million", "milliards", "milliard"] {
            let j = self.espace(k + nom.len());
            if self.commence(k, nom) && self.fin_de_mot(k + nom.len()) && self.car(j) == Some('$') {
                return Some((j + 1, format!("{} {nom} de dollars", n.en_mots(false))));
            }
        }
        // « 2 k$ » : deux mille dollars.
        if let Some(m) = ["k$", "k $"].iter().find(|m| self.commence(k, m))
            && let Some(e) = n.valeur()
            && n.points.is_empty()
            && n.decimales.as_ref().is_none_or(|d| d.len() <= 3)
        {
            let d = n.decimales.clone().unwrap_or_default();
            let total = e * 1000 + format!("{d:0<3}").parse::<u64>().unwrap_or(0);
            return Some((k + m.chars().count(), format!("{} dollar{}", en_lettres(total, false), s(total >= 2))));
        }
        match self.car(k) {
            Some('$') => return Some(self.par(k + 1, montant(n, Devise::Dollar))),
            Some('€') => return Some(self.par(k + 1, montant(n, Devise::Euro))),
            Some('¢') => return Some((k + 1, format!("{} cènte", n.en_mots(false)))),
            Some('%') => return Some((k + 1, format!("{} pour cent", n.en_mots(false)))),
            Some('°') => {
                let j = self.espace(k + 1);
                let (fin, echelle) = match self.car(j) {
                    Some('C') if self.fin_de_mot(j + 1) => (j + 1, " Celsius"),
                    Some('F') if self.fin_de_mot(j + 1) => (j + 1, " Fahrenheit"),
                    _ => (k + 1, ""),
                };
                return Some((fin, format!("{} degré{}{echelle}", n.en_mots(false), s(n.pluriel()))));
            }
            _ => {}
        }
        // « 937.05 dollars » : un montant, même écrit avec un point.
        if n.decimales.as_ref().is_some_and(|d| d.len() == 2)
            && let Some(mot) =
                ["dollars", "dollar"].iter().find(|m| self.commence(k, m) && self.fin_de_mot(k + m.len()))
        {
            return Some((k + mot.len(), montant(n, Devise::Dollar)));
        }
        for (motif, singulier, pluriel, feminin) in UNITES_DE_MESURE {
            // « 14 h » entier est une heure (déjà lue) ; « 1,5 h » une durée.
            if motif == "h" && n.entier() {
                continue;
            }
            let fin = k + motif.chars().count();
            if self.commence(k, motif) && self.fin_de_mot(fin) && !(self.car(fin) == Some('.') && self.lettre(fin + 1))
            {
                let nom = if n.pluriel() { pluriel } else { singulier };
                return Some(self.par(fin, format!("{} {nom}", n.en_mots(feminin))));
            }
        }
        None
    }

    /// Ce qui suit une unité ou un montant après une barre : « 12 $/h » (de l'heure), « 150 $/pi² » (du pied carré),
    /// « 40 $/mois » (par mois).
    fn par(&self, j: usize, dit: String) -> (usize, String) {
        if self.car(j) != Some('/') {
            return (j, dit);
        }
        for (motif, suite) in [("h", "de l'heure"), ("pi²", "du pied carré"), ("pi2", "du pied carré")]
            .into_iter()
            .chain([("m²", "du mètre carré"), ("m2", "du mètre carré")])
        {
            if self.commence(j + 1, motif) && self.fin_de_mot(j + 1 + motif.chars().count()) {
                return (j + 1 + motif.chars().count(), format!("{dit} {suite}"));
            }
        }
        let mut fin = j + 1;
        while self.lettre(fin) {
            fin += 1;
        }
        if fin == j + 1 {
            return (j, dit);
        }
        let mot: String = self.c[j + 1..fin].iter().collect();
        (fin, format!("{dit} par {mot}"))
    }

    /// Un numéro de téléphone : « (450) 555-1234 », « 450 555-1234 », « 1 800 555-1234 », « 555-1234 » — ses groupes,
    /// chiffre à chiffre : « quatre cinq zéro, cinq cinq cinq, un deux trois quatre ».
    fn telephone(&self, k: usize) -> Option<(usize, String)> {
        let separe = |j: usize| matches!(self.car(j), Some(' ' | '-' | '.'));
        let mut groupes = Vec::new();
        let mut j = k;
        if self.commence(j, "+1") && separe(j + 2) {
            groupes.push("1".to_string());
            j += 3;
        } else if self.car(j) == Some('1')
            && separe(j + 1)
            && (self.chiffres(j + 2) == 3 || self.car(j + 2) == Some('('))
        {
            groupes.push("1".to_string());
            j += 2;
        }
        // L'indicatif régional, entre parenthèses ou non.
        if self.car(j) == Some('(') && self.chiffres(j + 1) == 3 && self.car(j + 4) == Some(')') {
            groupes.push(self.c[j + 1..j + 4].iter().collect());
            j = self.espace(j + 5);
        } else if self.chiffres(j) == 3 && separe(j + 3) && self.chiffres(j + 4) == 3 {
            groupes.push(self.c[j..j + 3].iter().collect());
            j += 4;
        } else if !groupes.is_empty() {
            return None;
        }
        // Le numéro : trois chiffres, un séparateur (un tiret seul s'il n'y a pas d'indicatif), quatre chiffres.
        let tiret = self.car(j + 3) == Some('-');
        if self.chiffres(j) != 3 || !separe(j + 3) || (groupes.is_empty() && !tiret) || self.chiffres(j + 4) != 4 {
            return None;
        }
        let fin = j + 8;
        if self.chiffre(fin) || (matches!(self.car(fin), Some(',' | '.')) && self.chiffre(fin + 1)) {
            return None;
        }
        groupes.push(self.c[j..j + 3].iter().collect());
        groupes.push(self.c[j + 4..j + 8].iter().collect());
        Some((fin, groupes.iter().map(|g| chiffre_a_chiffre(g)).collect::<Vec<_>>().join(", ")))
    }

    // ---- Les mots ------------------------------------------------------------------------------------------------

    /// Un mot, une abréviation ou une adresse.
    fn mot(&mut self) -> bool {
        let k = self.i;
        if !self.lettre(k) {
            return false;
        }
        let (fin, dit) = self
            .abreviation(k)
            .or_else(|| self.initiales(k))
            .or_else(|| self.romain(k))
            .or_else(|| self.adresse(k))
            .or_else(|| self.homographe(k))
            .unwrap_or_else(|| self.mot_seul(k));
        self.dire(&dit);
        self.i = fin;
        self.apres_un_nombre = false;
        true
    }

    /// Une abréviation (« M. », « boul. », « c.-à-d. », « etc. »), ou une expression du lexique (« Black Friday ») ;
    /// derrière une élision aussi (« d'env. »).
    fn abreviation(&self, k: usize) -> Option<(usize, String)> {
        let mut j = k;
        while self.lettre(j) {
            j += 1;
        }
        let prefixe: String = self.c[k..j].iter().collect();
        if matches!(self.car(j), Some('\'' | '’')) && ELISIONS.contains(&prefixe.to_lowercase().as_str()) {
            let (fin, dit) = self.abreviation_seule(j + 1)?;
            return Some((fin, format!("{prefixe}{}{dit}", self.c[j])));
        }
        self.abreviation_seule(k)
    }

    fn abreviation_seule(&self, k: usize) -> Option<(usize, String)> {
        for (motif, dit, suite) in ABREVIATIONS {
            let fin = k + motif.chars().count();
            let ok = self.commence(k, motif)
                && match suite {
                    Suite::Tout => motif.ends_with(['.', ':']) || self.fin_de_mot(fin),
                    Suite::Majuscule => self.car(fin).is_some_and(char::is_uppercase),
                    Suite::Chiffre => self.chiffre(fin),
                };
            if ok {
                return Some((fin, dit.to_string()));
            }
        }
        // « etc. » : son point finit aussi la phrase quand une autre commence (ou que rien ne suit).
        if self.commence(k, "etc.") {
            let mut j = k + 4;
            while self.car(j) == Some(' ') {
                j += 1;
            }
            let phrase_finie = self.car(j).is_none_or(|c| c.is_uppercase() || c == '\n');
            return Some((k + 4, if phrase_finie { "ètcétéra." } else { "ètcétéra" }.to_string()));
        }
        EXPRESSIONS.iter().find_map(|(motif, dit)| {
            let fin = k + motif.chars().count();
            (self.commence(k, motif) && self.fin_de_mot(fin)).then(|| (fin, dit.to_string()))
        })
    }

    /// Des initiales (« A.B. », « J.-L. ») : leurs lettres épelées, sans leurs points — eSpeak continuait d'épeler le
    /// mot qui suit (« A.B. y est notée » : « A B i grec »). Le dernier point reste s'il finit le texte.
    fn initiales(&self, k: usize) -> Option<(usize, String)> {
        let mut lettres = Vec::new();
        let mut j = k;
        while self.car(j).is_some_and(char::is_uppercase) && self.car(j + 1) == Some('.') {
            lettres.push(self.c[j].to_string());
            j += 2;
            if self.car(j) == Some('-') && self.car(j + 1).is_some_and(char::is_uppercase) {
                j += 1;
            }
        }
        if lettres.len() < 2 || self.lettre(j) {
            return None;
        }
        let mut dit = lettres.join(" ");
        if self.c[j..].iter().all(|c| c.is_whitespace()) {
            dit.push('.');
        }
        Some((j, dit))
    }

    /// Les homographes qu'eSpeak lit d'une seule façon, là où le sens en demande une autre. « En fait » au sens de « en
    /// réalité », et « au fait », se disent avec leur t (« faite ») — pas « on en fait », « il s'en fait » : le verbe.
    /// « En plus », « de plus », « le plus » et « au plus » au bout d'un groupe de mots, et « en plus de », avec leur s
    /// (« plusse ») — pas « de plus en plus », ni « plus » devant un adjectif ; « plus ou moins » avec sa liaison
    /// (« plus-ou-moins »).
    fn homographe(&self, k: usize) -> Option<(usize, String)> {
        let mut j = k;
        while self.lettre(j) {
            j += 1;
        }
        // Un mot composé (« plus-value »), une élision : le mot seul les lit.
        if matches!(self.car(j), Some('-' | '\'' | '’')) && self.lettre(j + 1) {
            return None;
        }
        let mot: String = self.c[k..j].iter().collect();
        let casse = |dit: &str| {
            let mut d = dit.chars();
            match d.next() {
                Some(p) if mot.starts_with(char::is_uppercase) => p.to_uppercase().chain(d).collect(),
                _ => dit.to_string(),
            }
        };
        let fin_de_groupe = self.c[j..].iter().find(|c| !c.is_whitespace()).is_none_or(|c| ",.;:!?…".contains(*c));
        let (d, f) = self.bornes_du_mot_avant(k);
        let avant: String = self.c[d..f].iter().collect::<String>().to_lowercase();
        // Le mot d'avant celui-là ; s'il est élidé (« qu'en », « s'en »), ce qui l'élide.
        let elide = d > 0 && matches!(self.c[d - 1], '\'' | '’');
        let (d2, f2) = self.bornes_du_mot_avant(if elide { d - 1 } else { d });
        let avant_avant: String = self.c[d2..f2].iter().collect::<String>().to_lowercase();
        let rien_avant = d2 == f2;
        match mot.to_lowercase().as_str() {
            "plus" => {
                let m = self.espace(j);
                if self.commence(m, "ou moins") && self.fin_de_mot(m + 8) {
                    return Some((m + 8, casse("plus-ou-moins")));
                }
                if avant_avant == "plus" {
                    return None;
                }
                let apres = self.mot_apres(j);
                let au_bout = matches!(avant.as_str(), "en" | "de" | "le" | "au") && fin_de_groupe;
                let en_plus_de = avant == "en" && matches!(apres.as_str(), "de" | "d" | "du" | "des");
                (au_bout || en_plus_de).then(|| (j, casse("plusse")))
            }
            "fait" if avant == "en" => {
                // « on en fait », « qui en fait partie », « il s'en fait », « il n'en fait rien » : le verbe faire.
                const SUJETS: [&str; 11] =
                    ["je", "tu", "il", "elle", "on", "nous", "vous", "ils", "elles", "ça", "qui"];
                const DEVANT: [&str; 11] =
                    ["mais", "et", "donc", "car", "or", "alors", "est", "sont", "était", "a", "ont"];
                let adverbe = if elide {
                    matches!(avant_avant.as_str(), "qu" | "lorsqu" | "puisqu" | "jusqu")
                } else {
                    !SUJETS.contains(&avant_avant.as_str())
                        && (rien_avant || DEVANT.contains(&avant_avant.as_str()) || fin_de_groupe)
                };
                adverbe.then(|| (j, casse("faite")))
            }
            "fait" if avant == "au" => {
                // « au fait de » : au courant de.
                let apres = self.mot_apres(j);
                (!matches!(apres.as_str(), "de" | "d" | "du" | "des")).then(|| (j, casse("faite")))
            }
            _ => None,
        }
    }

    /// Un ordinal en chiffres romains : « XXIe siècle », « XIXe » (deux lettres au moins : « Le » et « Ce » sont des
    /// mots).
    fn romain(&self, k: usize) -> Option<(usize, String)> {
        let mut j = k;
        while self.car(j).is_some_and(|c| "IVXLCDM".contains(c)) {
            j += 1;
        }
        if j - k < 2 {
            return None;
        }
        let v = valeur_romaine(&self.c[k..j].iter().collect::<String>())?;
        let s = ["èmes", "ème", "es", "e"]
            .iter()
            .find(|s| self.commence(j, s) && self.fin_de_mot(j + s.chars().count()))?;
        let pluriel = if s.ends_with('s') { "s" } else { "" };
        Some((j + s.chars().count(), format!("{}{pluriel}", ordinal(v, false))))
    }

    /// Une adresse courriel (« info@exemple.ca ») ou un site (« exemple.com ») : « info arobase exemple point C A ».
    fn adresse(&self, k: usize) -> Option<(usize, String)> {
        let mut j = k;
        while self.car(j).is_some_and(|x| x.is_ascii_alphanumeric() || "._%+-@".contains(x)) {
            j += 1;
        }
        while j > k && matches!(self.c[j - 1], '.' | '-') {
            j -= 1;
        }
        let jeton: String = self.c[k..j].iter().collect();
        let (local, domaine) = match jeton.split_once('@') {
            Some((l, d)) => (Some(l), d),
            None => (None, jeton.as_str()),
        };
        let parties: Vec<&str> = domaine.split('.').collect();
        let fin_connue = DOMAINES.iter().find(|(d, _)| parties.last().is_some_and(|p| p.to_lowercase() == *d));
        let partie_valide = |p: &&str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        let valide = parties.len() >= 2
            && parties.iter().all(partie_valide)
            && match local {
                Some(l) => !l.is_empty() && !l.contains('@') && parties.last().is_some_and(|p| p.len() >= 2),
                None => fin_connue.is_some(),
            };
        if !valide || !self.fin_de_mot(j) {
            return None;
        }
        let mut dit = String::new();
        if let Some(l) = local {
            let l = l.replace('.', " point ").replace('_', " tiret bas ").replace('-', " tiret ");
            dit = tel_qu_il_se_dit(&l) + " arobase ";
        }
        let parties: Vec<String> = parties
            .iter()
            .map(|p| match DOMAINES.iter().find(|(d, _)| p.to_lowercase() == *d) {
                Some((_, lu)) => lu.to_string(),
                None => mot_dit(p),
            })
            .collect();
        dit.push_str(&parties.join(" point "));
        Some((j, dit))
    }

    /// Un mot entier (ses apostrophes et ses traits d'union compris), tel qu'il se dit ([`mot_dit`]).
    fn mot_seul(&self, k: usize) -> (usize, String) {
        let mut j = k;
        while self.lettre(j) || (matches!(self.car(j), Some('\'' | '’' | '-')) && self.lettre(j + 1)) {
            j += 1;
        }
        let mot: String = self.c[k..j].iter().collect();
        (j, mot_dit(&mot))
    }

    // ---- Les signes ----------------------------------------------------------------------------------------------

    /// Un tiret entre deux nombres (« 3-5 jours », « 9 h-17 h »), des points de suspension, une esperluette, un signe
    /// d'égalité, une fraction écrite d'un signe, une flèche ; une incise entre parenthèses ou entre deux tirets
    /// simples.
    fn signe(&mut self) -> bool {
        let k = self.i;
        let x = self.c[k];
        if matches!(x, '-' | '–' | '—') && self.apres_un_nombre {
            let j = self.espace(k + 1);
            if self.chiffre(j) {
                if !self.sortie.ends_with(' ') {
                    self.sortie.push(' ');
                }
                self.sortie.push_str("à ");
                self.i = j;
                return true;
            }
        }
        // Une flèche (« A → B », « -> », « => ») : une pause, comme une virgule.
        let fleche = match x {
            '→' | '⇒' => 1,
            '-' | '=' if self.car(k + 1) == Some('>') => 2,
            _ => 0,
        };
        if fleche > 0 {
            self.virgule_d_incise();
            self.i = k + fleche;
            self.apres_un_nombre = false;
            return true;
        }
        if self.incise(k) {
            self.apres_un_nombre = false;
            return true;
        }
        // Les points de suspension : une pause dans la phrase (une virgule ; eSpeak n'en fait aucune), ou sa fin.
        let suspension = if x == '…' {
            1
        } else if self.commence(k, "...") {
            3
        } else {
            0
        };
        let (fin, dit) = if suspension > 0 {
            let mut j = k + suspension;
            while self.car(j) == Some(' ') {
                j += 1;
            }
            let suite = self.car(j).is_some_and(|c| c.is_lowercase() || c.is_ascii_digit());
            (k + suspension, if suite { "," } else { "." })
        } else {
            let dit = match x {
                '&' => "et",
                '+' => "plusse",
                '=' => "égale",
                '≈' => "environ",
                '§' => "paragraphe",
                '½' => "un demi",
                '¼' => "un quart",
                '¾' => "trois quarts",
                '⅓' => "un tiers",
                '⅔' => "deux tiers",
                _ => return false,
            };
            (k + 1, dit)
        };
        if suspension == 0 {
            if self.sortie.ends_with(|c: char| !c.is_whitespace()) {
                self.sortie.push(' ');
            }
            self.sortie.push_str(dit);
            if self.car(fin).is_some_and(|c| !c.is_whitespace()) {
                self.sortie.push(' ');
            }
        } else {
            self.sortie.push_str(dit);
        }
        self.i = fin;
        self.apres_un_nombre = false;
        true
    }

    /// Une incise : eSpeak lit d'un trait ce qui est entre parenthèses (« Le prix (avant taxes) est bon. ») ou entre
    /// deux tirets simples (« Factures - Novembre »), sans y voir de fin de proposition ; entre virgules, Piper s'y
    /// arrête, comme il le fait déjà d'un tiret long (« — », « – »). La marque collée à un mot (« client(s) »,
    /// « le(s) ») se tait : eSpeak la lirait « ès ».
    fn incise(&mut self, k: usize) -> bool {
        match self.c[k] {
            '(' => {
                let n = self.c[k + 1..].iter().take_while(|c| c.is_alphabetic()).count();
                if k > 0 && self.lettre(k - 1) && (1..=3).contains(&n) && self.car(k + 1 + n) == Some(')') {
                    self.i = k + 2 + n;
                    return true;
                }
                self.virgule_d_incise();
                if !self.sortie.is_empty() {
                    self.sortie.push(' ');
                }
                self.i = k + 1;
                while self.car(self.i) == Some(' ') {
                    self.i += 1;
                }
                true
            }
            ')' => {
                let suite = self.c[k + 1..].iter().find(|c| **c != ' ').copied();
                let fin = self.sortie.trim_end().len();
                self.sortie.truncate(fin);
                if suite.is_some_and(|c| c.is_alphanumeric() || matches!(c, '«' | '"' | '(')) {
                    self.sortie.push(',');
                }
                self.i = k + 1;
                true
            }
            '-' if k > 0 && self.car(k - 1) == Some(' ') && self.car(k + 1) == Some(' ') => {
                self.virgule_d_incise();
                self.i = k + 1;
                true
            }
            _ => false,
        }
    }

    /// La virgule qui ouvre une incise, collée au mot qui la précède — sauf après une ponctuation qui fait déjà la
    /// pause.
    fn virgule_d_incise(&mut self) {
        let fin = self.sortie.trim_end().len();
        self.sortie.truncate(fin);
        if self.sortie.ends_with(|c: char| c.is_alphanumeric() || matches!(c, '»' | '"' | '\'' | '’')) {
            self.sortie.push(',');
        }
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    fn dit(texte: &str) -> String {
        tel_qu_il_se_dit(texte)
    }

    #[test]
    fn les_nombres_se_disent_en_lettres() {
        let cas = [
            (0, "zéro"),
            (1, "un"),
            (16, "seize"),
            (17, "dix-sept"),
            (21, "vingt et un"),
            (22, "vingt-deux"),
            (70, "soixante-dix"),
            (71, "soixante et onze"),
            (77, "soixante-dix-sept"),
            (80, "quatre-vingts"),
            (81, "quatre-vingt-un"),
            (91, "quatre-vingt-onze"),
            (99, "quatre-vingt-dix-neuf"),
            (100, "cent"),
            (101, "cent un"),
            (200, "deux cents"),
            (250, "deux cent cinquante"),
            (1000, "mille"),
            (1001, "mille un"),
            (1990, "mille neuf cent quatre-vingt-dix"),
            (2026, "deux mille vingt-six"),
            (80_000, "quatre-vingt mille"),
            (200_000, "deux cent mille"),
            (1_234_567, "un million deux cent trente-quatre mille cinq cent soixante-sept"),
            (200_000_000, "deux cents millions"),
            (2_000_000_000, "deux milliards"),
        ];
        for (n, attendu) in cas {
            assert_eq!(en_lettres(n, false), attendu, "{n}");
        }
        assert_eq!(en_lettres(21, true), "vingt et une");
        assert_eq!(en_lettres(81, true), "quatre-vingt-une");
        assert_eq!(en_lettres(1_000_000, true), "un million");
        assert_eq!(ordinal(1, true), "première");
        assert_eq!(ordinal(2, false), "deuxième");
        assert_eq!(ordinal(5, false), "cinquième");
        assert_eq!(ordinal(9, false), "neuvième");
        assert_eq!(ordinal(21, false), "vingt et unième");
        assert_eq!(ordinal(80, false), "quatre-vingtième");
        assert_eq!(ordinal(100, false), "centième");
        assert_eq!(ordinal(1000, false), "millième");
    }

    #[test]
    fn les_heures_se_disent() {
        assert_eq!(dit("Rendez-vous à 14 h 30."), "Rendez-vous à quatorze heures trente.");
        assert_eq!(dit("Le chantier commence à 9 h."), "Le chantier commence à neuf heures.");
        assert_eq!(dit("On se voit à 13h30."), "On se voit à treize heures trente.");
        assert_eq!(dit("Vers 8 h 05, puis 21 h."), "Vers huit heures cinq, puis vingt et une heures.");
        assert_eq!(dit("À 1 h du matin, 0 h 15."), "À une heure du matin, zéro heure quinze.");
        assert_eq!(dit("La réunion est à 14:30."), "La réunion est à quatorze heures trente.");
        assert_eq!(dit("Il reste 2 h 30 min."), "Il reste deux heures trente minutes.");
        assert_eq!(
            dit("De 9 h à 17 h, ou 9 h-17 h."),
            "De neuf heures à dix-sept heures, ou neuf heures à dix-sept heures."
        );
        assert_eq!(dit("Il est 13 h 21."), "Il est treize heures vingt et une.");
        assert_eq!(
            dit("Ça prend 15 min et 30 s, ou 1,5 h."),
            "Ça prend quinze minutes et trente secondes, ou un virgule cinq heure."
        );
        // « 14 heures » est déjà en mots ; « 4 h. » finit la phrase.
        assert_eq!(dit("À 14 heures, ou 4 h."), "À quatorze heures, ou quatre heures.");
    }

    #[test]
    fn les_montants_se_disent_comme_au_quebec() {
        assert_eq!(dit("Le total est de 250 $."), "Le total est de deux cent cinquante dollars.");
        assert_eq!(dit("Ça coûte 0,04 $ par demande."), "Ça coûte quatre cènte par demande.");
        assert_eq!(dit("La facture : 1 250,50 $."), "La facture : mille deux cent cinquante dollars cinquante.");
        assert_eq!(dit("Le prix : 12,99 $, ou 1 $."), "Le prix : douze dollars quatre-vingt-dix-neuf, ou un dollar.");
        assert_eq!(dit("Une maison de 500 000 $."), "Une maison de cinq cent mille dollars.");
        assert_eq!(dit("Il manque 350 à 550 $."), "Il manque trois cent cinquante à cinq cent cinquante dollars.");
        assert_eq!(dit("Un budget de 1,5 M$."), "Un budget de un virgule cinq million de dollars.");
        assert_eq!(dit("Environ 2 k$ de plus."), "Environ deux mille dollars de plusse.");
        assert_eq!(dit("Le prix est de $250."), "Le prix est de deux cent cinquante dollars.");
        assert_eq!(dit("Le solde : -250 $."), "Le solde : moins deux cent cinquante dollars.");
        assert_eq!(dit("Le montant est de 937.05 dollars."), "Le montant est de neuf cent trente-sept dollars cinq.");
        assert_eq!(dit("Un coût de 0,004 $."), "Un coût de zéro virgule zéro zéro quatre dollar.");
        assert_eq!(dit("Deux millions : 2 000 000 $."), "Deux millions : deux millions de dollars.");
        assert_eq!(dit("Un tarif de 85 $/h."), "Un tarif de quatre-vingt-cinq dollars de l'heure.");
        assert_eq!(dit("Ça fait 25 ¢ et 20 €."), "Ça fait vingt-cinq cènte et vingt euros.");
        // Les espaces insécables des milliers.
        assert_eq!(dit("Soit 58\u{202f}500\u{a0}$."), "Soit cinquante-huit mille cinq cents dollars.");
    }

    #[test]
    fn les_unites_et_les_signes_se_disent() {
        assert_eq!(dit("Il fera -3 °C, puis 5 °C."), "Il fera moins trois degrés Celsius, puis cinq degrés Celsius.");
        assert_eq!(dit("Des rafales de 70 km/h."), "Des rafales de soixante-dix kilomètres à l'heure.");
        assert_eq!(dit("Baissée de 5 %, fidèle à 100 %."), "Baissée de cinq pour cent, fidèle à cent pour cent.");
        assert_eq!(
            dit("Il faut 1,3 Go et 410 Mo."),
            "Il faut un virgule trois gigaoctet et quatre cent dix mégaoctets."
        );
        assert_eq!(dit("Une pièce de 12 pi sur 14 pi."), "Une pièce de douze pieds sur quatorze pieds.");
        assert_eq!(dit("Une surface de 1 200 pi²."), "Une surface de mille deux cents pieds carrés.");
        assert_eq!(dit("Un 2 x 4 de 8 pi, un 2x6."), "Un deux par quatre de huit pieds, un deux par six.");
        assert_eq!(dit("Un mur de 2,4 m de haut."), "Un mur de deux virgule quatre mètres de haut.");
        assert_eq!(
            dit("Une bouteille de 2 L, 1 tonne, 3,5 t."),
            "Une bouteille de deux litres, une tonne, trois virgule cinq tonnes."
        );
        assert_eq!(
            dit("La version 2.1.3 et la vue 360°."),
            "La version deux point un point trois et la vue trois cent soixante degrés."
        );
        // « 2 l'heure » : un « l » qui n'est pas des litres.
        assert_eq!(dit("Le 2 l'a dit."), "Le deux l'a dit.");
        assert_eq!(dit("Environ ~ 30, soit ≈ 40."), "Environ environ trente, soit environ quarante.");
        assert_eq!(dit("Le ticket #3 : 2 + 2 = 4."), "Le ticket numéro trois : deux plusse deux égale quatre.");
    }

    #[test]
    fn les_nombres_s_accordent_et_se_lisent_selon_leur_forme() {
        assert_eq!(
            dit("Il y a 1 personne, 21 personnes, 1 fois."),
            "Il y a une personne, vingt et une personnes, une fois."
        );
        assert_eq!(
            dit("Il y a 984 messages, dont 2 non lus."),
            "Il y a neuf cent quatre-vingt-quatre messages, dont deux non lus."
        );
        assert_eq!(dit("Toutes les 1,5 seconde, ou 0,7."), "Toutes les un virgule cinq seconde, ou zéro virgule sept.");
        assert_eq!(dit("Rafraîchis avec F5, la v17, en 5G."), "Rafraîchis avec F cinq, la v dix-sept, en cinq G.");
        assert_eq!(
            dit("Le code 007, le numéro 1234567890."),
            "Le code zéro zéro sept, le numéro un deux trois quatre cinq six sept huit neuf zéro."
        );
        assert_eq!(dit("De 3-5 jours, 10 – 15 min."), "De trois à cinq jours, dix à quinze minutes.");
        assert_eq!(
            dit("Les 3/4, la 1/2, 8/10, 24/7."),
            "Les trois quarts, la un demi, huit sur dix, vingt-quatre sur sept."
        );
        assert_eq!(dit("Je dirais 95 sur 100."), "Je dirais quatre-vingt-quinze sur cent.");
    }

    #[test]
    fn les_dates_et_les_ordinaux_se_disent() {
        assert_eq!(dit("Mise à jour le 2026-10-26."), "Mise à jour le vingt-six octobre deux mille vingt-six.");
        assert_eq!(
            dit("Le relevé du 26/10/2026, et le 1/11."),
            "Le relevé du vingt-six octobre deux mille vingt-six, et le premier novembre."
        );
        assert_eq!(dit("La date limite est le 26/10."), "La date limite est le vingt-six octobre.");
        assert_eq!(dit("Le 1er novembre, en 2026-10."), "Le premier novembre, en octobre deux mille vingt-six.");
        assert_eq!(dit("Le 1er étage, la 1re fois, la 1ère."), "Le premier étage, la première fois, la première.");
        assert_eq!(
            dit("Au 2e, au 3e, la 2ème, le 21e, les 2es."),
            "Au deuxième, au troisième, la deuxième, le vingt et unième, les deuxièmes."
        );
        assert_eq!(dit("Le 2nd et la 2nde."), "Le second et la seconde.");
        assert_eq!(
            dit("Le XXIe siècle, les XIXes. Le roi. Ce IIIIe."),
            "Le vingt et unième siècle, les dix-neuvièmes. Le roi. Ce IIIIe."
        );
        assert_eq!(
            dit("Le n° 12, le no 4, l'article § 3."),
            "Le numéro douze, le numéro quatre, l'article paragraphe trois."
        );
        // Une plage d'années n'est pas une date.
        assert_eq!(dit("De 2025-2026."), "De deux mille vingt-cinq à deux mille vingt-six.");
    }

    #[test]
    fn les_telephones_se_disent_chiffre_a_chiffre() {
        let groupes = "quatre cinq zéro, cinq cinq cinq, un deux trois quatre";
        assert_eq!(dit("Appelle au 450 555-1234."), format!("Appelle au {groupes}."));
        assert_eq!(dit("Appelle au (450) 555-1234."), format!("Appelle au {groupes}."));
        assert_eq!(dit("Appelle au 450-555-1234."), format!("Appelle au {groupes}."));
        assert_eq!(
            dit("Sans frais : 1 800 555-1234."),
            "Sans frais : un, huit zéro zéro, cinq cinq cinq, un deux trois quatre."
        );
        assert_eq!(dit("Le 555-1234."), "Le cinq cinq cinq, un deux trois quatre.");
        // Pas un numéro : des milliers, une plage.
        assert_eq!(
            dit("Les 450 555 personnes."),
            "Les quatre cent cinquante mille cinq cent cinquante-cinq personnes."
        );
    }

    #[test]
    fn les_abreviations_se_disent_sans_couper_la_phrase() {
        assert_eq!(dit("J'ai parlé à M. Tremblay."), "J'ai parlé à monsieur Tremblay.");
        assert_eq!(dit("Mme Gagnon et Mlle Côté."), "madame Gagnon et mademoiselle Côté.");
        assert_eq!(dit("Le Dr Roy, la Dre Roy."), "Le docteur Roi, la docteure Roi.");
        assert_eq!(dit("Le notaire, Me Lavoie. Me voici."), "Le notaire, maître Lavoie. Me voici.");
        assert_eq!(dit("St-Jean, Ste-Julie, Saint-Hyacinthe."), "Saint-Jean, Sainte-Julie, Saint-Iacinte.");
        assert_eq!(
            dit("Le plan, c.-à-d. le dessin, p. ex. le gypse."),
            "Le plan, c'est-à-dire le dessin, par exemple le gypse."
        );
        assert_eq!(
            dit("Au 123, boul. René-Lévesque, app. 4."),
            "Au cent vingt-trois, boulevard René-Lévesque, appartement quatre."
        );
        assert_eq!(dit("Tél. : 450 555-1234."), "téléphone : quatre cinq zéro, cinq cinq cinq, un deux trois quatre.");
        assert_eq!(dit("Le bois, le gypse, etc. Puis le reste."), "Le bois, le gypse, ètcétéra. Puis le reste.");
        assert_eq!(dit("Le bois, etc., puis le reste, etc."), "Le bois, ètcétéra, puis le reste, ètcétéra.");
        assert_eq!(
            dit("Le plan A vs le plan B, d'env. 200 $."),
            "Le plan A versus le plan B, d'environ deux cents dollars."
        );
        assert_eq!(dit("Exemple ltée, svp."), "Exemple limitée, s'il vous plaît.");
        // Pas des abréviations : « Drummondville », « app » sans numéro, « Mmm ».
        assert_eq!(dit("À Drummondville, l'app. Mmm."), "À Drummondville, l'app. Mmm.");
    }

    #[test]
    fn les_sigles_et_les_mots_anglais_se_disent_en_francais() {
        assert_eq!(dit("Les données sont dans l'ERP."), "Les données sont dans l'E R P.");
        assert_eq!(dit("L'IA t'aide, et la SAAQ aussi."), "l'I A t'aide, et la S A A Q aussi.");
        assert_eq!(dit("La TPS, la TVQ, le CLSC, OK."), "La TPS, la TVQ, le CLSC, OK.");
        assert_eq!(dit("Le CPE et l'ETS, le VUS."), "Le C P E et l'E T S, le V U S.");
        assert_eq!(dit("Le rythme, rythmé, Rythmes."), "Le ritme, ritmé, Ritmes.");
        assert_eq!(dit("Le REER, le CELI et le JSON."), "Le Èrééèr, le Céli et le Djéssonne.");
        assert_eq!(dit("Ouvre Outlook, puis ta boîte Gmail."), "Ouvre Aoutlouk, puis ta boîte Djimèle.");
        assert_eq!(dit("Le point d'aujourd'hui, et jusqu'aujourd'hui."), "Le point d'aujourdhui, et jusqu'aujourdhui.");
        assert_eq!(dit("Je suis Eve, la voix d'Eve."), "Je suis Ève, la voix d'Ève.");
        assert_eq!(
            dit("Pour l'Halloween, des emails et un week-end."),
            "Pour l'Halloouine, des imèles et un ouikènde."
        );
        assert_eq!(dit("Une réunion sur Teams, un Black Friday."), "Une réunion sur Timmze, un Blac Fraïdé.");
        // Le serpent reste un serpent, et un mot inconnu passe tel quel.
        assert_eq!(dit("Un python et du Python."), "Un python et du Païtonne.");
        assert_eq!(dit("Claude, Opus, Excel, Chrome."), "Claude, Opus, Excel, Chrome.");
    }

    #[test]
    fn les_villes_au_nom_anglais_se_disent_comme_au_quebec() {
        assert_eq!(dit("La rue des Pins à Granby."), "La rue des Pins à Granbi.");
        assert_eq!(dit("De Waterloo à Sutton, puis Knowlton."), "De Ouatèrlou à Seuttonne, puis Noltonne.");
        assert_eq!(dit("Marieville, Coaticook et Saint-Hyacinthe."), "Mariville, Coaticouk et Saint-Iacinte.");
        // « Bury » et « Pike » ne se disent ainsi que nommés, avec leur majuscule.
        assert_eq!(dit("Bury et Pike River."), "Béri et Païk River.");
    }

    #[test]
    fn les_prononciations_de_la_personne_passent_avant() {
        let p: BTreeMap<String, String> =
            [("Granby", "Grannbi"), ("Saint-Lin", "Saint-Linne"), ("ayer's cliff", "Èrz Clif"), ("vide", " ")]
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect();
        let dit = |t: &str| avec_les_prononciations(t, &p);
        // Sans égard à la casse, la majuscule gardée ; en mots entiers seulement.
        assert_eq!(dit("À Granby, et granby."), "À Grannbi, et grannbi.");
        assert_eq!(dit("Granbyville et Granby2 restent."), "Granbyville et Granby2 restent.");
        // Quelques mots, l'apostrophe courbe comme la droite ; une graphie vide ne compte pas.
        assert_eq!(dit("Ayer’s Cliff, le vide."), "Èrz Clif, le vide.");
        assert_eq!(dit("Saint-Lin."), "Saint-Linne.");
        // Puis la diction : un mot de la personne n'est pas lu à nouveau par le lexique.
        assert_eq!(tel_qu_il_se_dit(&dit("Granby à 14 h.")), "Grannbi à quatorze heures.");
        assert_eq!(avec_les_prononciations("Granby.", &BTreeMap::new()), "Granby.");
    }

    #[test]
    fn les_adresses_et_les_suspensions_se_disent() {
        assert_eq!(dit("Écris à info@exemple.ca."), "Écris à info arobase exemple point C A.");
        assert_eq!(
            dit("Écris à sam.tremblay2000@gmail.com, merci."),
            "Écris à sam point tremblay deux mille arobase djimèle point com, merci."
        );
        assert_eq!(dit("Le site exemple.com."), "Le site exemple point com.");
        // Pas des adresses : un fichier, une abréviation.
        assert_eq!(dit("Le fichier rapport.pdf, N.B. le reste."), "Le fichier rapport.pdf, nota bene le reste.");
        assert_eq!(dit("Attends… je regarde."), "Attends, je regarde.");
        assert_eq!(dit("Je ne sais pas... Peut-être."), "Je ne sais pas. Peut-être.");
        assert_eq!(dit("Le bois & le gypse, ½ tasse."), "Le bois et le gypse, un demi tasse.");
        assert_eq!(dit("Le plan A → le plan B, de 2 -> 3, x => y."), "Le plan A, le plan B, de deux, trois, x, y.");
    }

    /// Un corpus (une phrase par ligne, ou « famille|phrase »), tel qu'il se dit : pour le faire lire à Piper, avant et
    /// après. `EVE_CORPUS=corpus.txt cargo test --release --lib -- --ignored --nocapture un_corpus_se_dit`.
    #[test]
    #[ignore]
    fn un_corpus_se_dit() {
        let Ok(chemin) = std::env::var("EVE_CORPUS") else { return };
        let texte = std::fs::read_to_string(chemin).expect("le corpus ne se lit pas");
        for l in texte.lines().filter(|l| !l.trim().is_empty()) {
            match l.split_once('|') {
                Some((famille, phrase)) => println!("{famille}|{}", dit(phrase)),
                None => println!("|{}", dit(l)),
            }
        }
    }

    #[test]
    fn le_texte_sans_rien_a_dire_autrement_passe_tel_quel() {
        let tel_quel = [
            "Bonjour Sam. Je t'ai préparé le point de la journée : deux rendez-vous, et une facture à envoyer.",
            "Bonne nouvelle : le client a accepté la soumission. On commence lundi !",
            "« Le chantier », dit-il ; puis — une pause.",
            "",
        ];
        for t in tel_quel {
            assert_eq!(dit(t), t);
        }
    }

    #[test]
    fn les_incises_se_detachent_entre_virgules() {
        assert_eq!(dit("Le prix (avant taxes) est bon."), "Le prix, avant taxes, est bon.");
        assert_eq!(dit("Il reste le 31 mars (impôt, retenues)."), "Il reste le trente et un mars, impôt, retenues.");
        assert_eq!(dit("La démo (D) et le plein écran (F) marchent."), "La démo, D, et le plein écran, F, marchent.");
        assert_eq!(dit("(Entre nous) c'est fait."), "Entre nous, c'est fait.");
        assert_eq!(dit("Le dossier « Factures - Novembre » est prêt."), "Le dossier « Factures, Novembre » est prêt.");
        // La marque du pluriel collée au mot se tait ; un tiret long, une plage, un numéro gardent leur lecture.
        assert_eq!(dit("Le ou les client(s) et le(s) projet(s)."), "Le ou les client et le projet.");
        assert_eq!(dit("Le chantier — celui de Sam — avance."), "Le chantier — celui de Sam — avance.");
        assert_eq!(dit("Il faut de 3 - 5 jours."), "Il faut de trois à cinq jours.");
        assert_eq!(dit("Puis (entre parenthèses) — une pause."), "Puis, entre parenthèses — une pause.");
    }

    #[test]
    fn les_homographes_se_disent_selon_leur_sens() {
        // « En fait », « au fait » : avec leur t ; le verbe faire garde le sien.
        assert_eq!(dit("En fait, c'est réglé. Mais en fait non."), "En faite, c'est réglé. Mais en faite non.");
        assert_eq!(
            dit("C'est en fait le seul, parce qu'en fait il pleut. Au fait, le courriel est parti."),
            "C'est en faite le seul, parce qu'en faite il pleut. Au faite, le courriel est parti."
        );
        let verbe = "Ce qu'on en fait, il s'en fait, il n'en fait rien, elle en fait partie, il est au fait du sujet.";
        assert_eq!(dit(verbe), verbe);
        // « plus » : [plys] au bout d'un groupe, devant « de », et pour le signe ; [ply] ailleurs.
        assert_eq!(dit("En plus, c'est rapide, en plus de ça."), "En plusse, c'est rapide, en plusse de ça.");
        assert_eq!(
            dit("Trois choses de plus. C'est ce que j'aime le plus, tout au plus."),
            "Trois choses de plusse. C'est ce que j'aime le plusse, tout au plusse."
        );
        assert_eq!(dit("Plus ou moins deux heures."), "Plus-ou-moins deux heures.");
        let ply = "De plus en plus, le plus grand, il n'y en a plus, en plus grand, une plus-value.";
        assert_eq!(dit(ply), ply);
        assert_eq!(dit("Il fera +5 °C, soit 2 + 3."), "Il fera plusse cinq degrés Celsius, soit deux plusse trois.");
        // « y » : le pronom, pas la lettre ; des initiales, épelées sans leurs points.
        assert_eq!(dit("Vas-y, allez-y, j'y vais."), "Vas-i, allez-i, j'y vais.");
        assert_eq!(dit("Toitures A.B. y est notée, comme J.-L. Roy."), "Toitures A B y est notée, comme J L Roi.");
        assert_eq!(dit("Chez Toitures R.B."), "Chez Toitures R B.");
        assert_eq!(dit("Un thé à la menthe, Stripe et Render."), "Un thé à la mante, Straïpe et Rènndeur.");
        assert_eq!(dit("Tom et Jerry."), "Tom et Djèri.");
        assert_eq!(dit("Le NEQ, le TDAH, chez IGA."), "Le N E Q, le T D A H, chez I G A.");
    }
}
