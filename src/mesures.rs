//! Les mesures : le temps de chaque étape d'une demande, de la fin de la parole à la première phrase de la réponse
//! entendue, et ce que coûte chaque tour de Claude. On mesure avant de changer, puis après, et on compare.
//!
//! Le chef note au journal (`eve.log`), pour chaque demande, une ligne « mesures du tour » ([`Temps::ligne`]) ; la fin
//! de chaque tour de Claude y dit déjà sa durée et son coût. `eve --mesures` relit le journal ([`lire_journal`]) et en
//! tire les médianes ([`releve`]), par version du programme (l'empreinte de sa source, notée à chaque démarrage), par
//! réglage de Claude (le modèle, l'effort) et par oreille (le modèle de Whisper, sur la carte graphique ou le
//! processeur) : deux séries ne se comparent que dans les mêmes conditions.
//!
//! Les étapes d'une demande, l'une après l'autre :
//! - **oreille** : de la fin de la parole (la touche relâchée, ou la phrase finie) au texte — la file de l'oreille et la
//!   transcription ; l'écoute continue attend en plus 0,7 s de silence pour savoir la phrase finie ;
//! - **attente** : le texte attend que Claude soit libre (une demande en cours, le démarrage) ;
//! - **premier mot** : Claude écrit le premier mot de sa réponse (après ses outils, s'il en prend) ;
//! - **première phrase** : … et finit sa première phrase, aussitôt donnée à la voix ;
//! - **voix** : la voix la fait entendre (la synthèse, le lecteur de Windows, après ce qu'elle disait déjà) ;
//! - **première parole** : en tout, de la fin de la parole au premier mot de la réponse entendu.
//!
//! Le coût d'un tour est la différence du cumul que rend Claude Code : son `total_cost_usd` est le coût de toute la
//! séance, que le journal notait jusqu'au 23 octobre 2026 comme celui du tour (n° 15 de l'analyse). Un journal d'avant
//! se relit quand même : la différence se fait ici.

use std::fmt::Write as _;

/// Une étape d'une demande, dans l'ordre où elles se suivent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Etape {
    Oreille,
    Attente,
    PremierMot,
    PremierePhrase,
    Voix,
    PremiereParole,
}

impl Etape {
    pub const TOUTES: [Etape; 6] =
        [Etape::Oreille, Etape::Attente, Etape::PremierMot, Etape::PremierePhrase, Etape::Voix, Etape::PremiereParole];

    /// Son nom, au journal et au relevé.
    pub fn nom(self) -> &'static str {
        match self {
            Etape::Oreille => "oreille",
            Etape::Attente => "attente",
            Etape::PremierMot => "premier mot",
            Etape::PremierePhrase => "première phrase",
            Etape::Voix => "voix",
            Etape::PremiereParole => "première parole",
        }
    }
}

/// Le début de la ligne des mesures d'une demande, au journal.
pub const LIGNE_MESURES: &str = "mesures du tour : ";

/// Les temps d'une demande, en secondes. Une étape qui n'a pas eu lieu manque : une demande écrite n'a pas d'oreille ;
/// en sourdine, ou la voix coupée, rien ne s'entend.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Temps([Option<f64>; Etape::TOUTES.len()]);

impl Temps {
    pub fn poser(&mut self, e: Etape, secondes: f64) {
        self.0[e as usize] = Some(secondes);
    }

    pub fn de(&self, e: Etape) -> Option<f64> {
        self.0[e as usize]
    }

    pub fn vide(&self) -> bool {
        self.0.iter().all(Option::is_none)
    }

    /// La ligne du journal : « mesures du tour : oreille 4.31 s, attente 0.00 s, premier mot 2.90 s, … ».
    pub fn ligne(&self) -> String {
        let etapes: Vec<String> =
            Etape::TOUTES.iter().filter_map(|&e| self.de(e).map(|s| format!("{} {s:.2} s", e.nom()))).collect();
        format!("{LIGNE_MESURES}{}", etapes.join(", "))
    }

    /// Relit une ligne du journal (le message, sans l'heure) ; `None` si ce n'en est pas une. Une étape illisible
    /// s'ignore : le reste compte.
    pub fn lire(message: &str) -> Option<Temps> {
        let reste = message.strip_prefix(LIGNE_MESURES)?;
        let mut t = Temps::default();
        for morceau in reste.split(", ") {
            let Some((nom, valeur)) = morceau.trim().strip_suffix(" s").and_then(|m| m.rsplit_once(' ')) else {
                continue;
            };
            let Some(e) = Etape::TOUTES.into_iter().find(|e| e.nom() == nom) else { continue };
            if let Ok(v) = valeur.parse::<f64>()
                && v.is_finite()
                && v >= 0.0
            {
                t.poser(e, v);
            }
        }
        Some(t)
    }
}

/// Le coût d'un tour : la différence du cumul que Claude Code rend à la fin de chaque tour, depuis le cumul du tour
/// d'avant dans la même séance. Au premier tour d'une séance (aucun cumul d'avant), ou si le cumul a baissé (une
/// séance neuve), le cumul lui-même.
pub fn cout_du_tour(cumul_d_avant: Option<f64>, cumul: f64) -> f64 {
    match cumul_d_avant {
        Some(avant) if cumul >= avant => cumul - avant,
        _ => cumul,
    }
}

/// La médiane (entre les deux du milieu, pour un nombre pair de valeurs) ; `None` sans valeur.
pub fn mediane(valeurs: &[f64]) -> Option<f64> {
    let v = triees(valeurs);
    let n = v.len();
    match n {
        0 => None,
        _ if n % 2 == 1 => Some(v[n / 2]),
        _ => Some((v[n / 2 - 1] + v[n / 2]) / 2.0),
    }
}

/// Le centile `p` (de 0 à 100), au rang le plus proche : la plus petite valeur qui laisse `p` % des valeurs à son
/// niveau ou en dessous.
pub fn centile(valeurs: &[f64], p: f64) -> Option<f64> {
    let v = triees(valeurs);
    if v.is_empty() {
        return None;
    }
    let rang = (p / 100.0 * v.len() as f64).ceil().max(1.0) as usize;
    Some(v[rang.min(v.len()) - 1])
}

fn triees(valeurs: &[f64]) -> Vec<f64> {
    let mut v: Vec<f64> = valeurs.iter().copied().filter(|x| x.is_finite()).collect();
    v.sort_by(f64::total_cmp);
    v
}

/// Ce que dit le journal pour une version du programme, un réglage de Claude et une oreille.
#[derive(Debug, Default, PartialEq)]
pub struct Serie {
    /// La version du programme et l'empreinte de sa source : « 0.1.0 (a1b2c3d) » (« 0.1.0 » avant les empreintes).
    pub version: String,
    /// Le modèle et l'effort de Claude : « claude-sonnet-5-5, effort medium ».
    pub claude: String,
    /// L'oreille : « Whisper small sur la carte graphique » ; vide tant qu'elle ne s'est pas dite.
    pub oreille: String,
    /// La première et la dernière minute de la série : « 2026-10-23 12:19 ».
    pub du: String,
    pub au: String,
    /// Les transcriptions : les secondes de parole, puis celles du calcul.
    pub transcriptions: Vec<(f64, f64)>,
    /// Les temps des demandes (« mesures du tour »).
    pub demandes: Vec<Temps>,
    /// Les tours de Claude qui répondaient à une demande : leur durée en secondes, et leur coût en dollars.
    pub tours: Vec<(Option<f64>, Option<f64>)>,
    /// Les tours silencieux (le préchauffage, le contexte gardé au chaud) : leur coût.
    pub silencieux: Vec<Option<f64>>,
}

/// Relit un journal d'Eve (`eve.log`) : ses séries, dans l'ordre où elles paraissent.
pub fn lire_journal(texte: &str) -> Vec<Serie> {
    let mut series: Vec<Serie> = Vec::new();
    let mut version = String::from("inconnue");
    // Le réglage de Claude n'est connu qu'à son lancement : une transcription faite avant attend la série.
    let mut claude: Option<String> = None;
    let mut en_suspens: Vec<(String, (f64, f64))> = Vec::new();
    // L'oreille se dit quelques secondes après chaque démarrage, avant toute transcription : jusque-là, elle garde la
    // valeur d'avant.
    let mut oreille = String::new();
    // Le cumul du tour d'avant, pour un journal d'avant les « cumul » ; il repart à chaque lancement de Claude Code.
    let mut cumul: Option<f64> = None;
    // Le dernier tour noté : la ligne de préchauffage qui le suit le dit silencieux.
    let mut dernier_tour: Option<usize> = None;
    for ligne in texte.lines() {
        let Some((minute, message)) = horodatee(ligne) else { continue };
        if let Some(v) = demarrage(message) {
            ranger_en_suspens(&mut series, &version, &oreille, &mut en_suspens);
            version = v.to_string();
            claude = None;
            cumul = None;
        } else if let Some(c) = message.strip_prefix("Claude Code lancé (").and_then(|r| r.rsplit_once(')')) {
            let c = c.0.trim_end_matches(", la conversation reprise").to_string();
            cumul = None;
            if !en_suspens.is_empty() {
                let i = serie(&mut series, &version, &c, &oreille, minute);
                for (m, t) in en_suspens.drain(..) {
                    series[i].transcriptions.push(t);
                    series[i].du = series[i].du.clone().min(m);
                }
            }
            claude = Some(c);
        } else if let Some(o) = message.strip_prefix("oreille : Whisper ") {
            // « small sur la carte graphique (port 8178, …) », « small sur le processeur, chargé en 3.2 s (…) ».
            oreille = format!("Whisper {}", o.split([',', '(']).next().unwrap_or("").trim());
        } else if let Some(t) = transcription(message) {
            match &claude {
                Some(c) => {
                    let i = serie(&mut series, &version, c, &oreille, minute);
                    series[i].transcriptions.push(t);
                }
                None => en_suspens.push((minute.to_string(), t)),
            }
        } else if let Some(t) = Temps::lire(message) {
            if !t.vide() {
                let i = serie(&mut series, &version, claude.as_deref().unwrap_or_default(), &oreille, minute);
                series[i].demandes.push(t);
            }
        } else if let Some((duree, cout, du_tour)) = fin_du_tour(message) {
            let cout = match cout {
                Some(c) if du_tour => Some(c),
                Some(c) => {
                    let t = cout_du_tour(cumul, c);
                    cumul = Some(c);
                    Some(t)
                }
                None => None,
            };
            let i = serie(&mut series, &version, claude.as_deref().unwrap_or_default(), &oreille, minute);
            series[i].tours.push((duree, cout));
            dernier_tour = Some(i);
        } else if message.starts_with("préchauffage fait") || message.starts_with("la base s'est branchée pendant") {
            if let Some(i) = dernier_tour.take()
                && let Some((_, cout)) = series[i].tours.pop()
            {
                series[i].silencieux.push(cout);
            }
        }
    }
    ranger_en_suspens(&mut series, &version, &oreille, &mut en_suspens);
    series
}

/// Les transcriptions d'une Eve dont Claude ne s'est jamais lancé : une série à part, à sa version.
fn ranger_en_suspens(
    series: &mut Vec<Serie>,
    version: &str,
    oreille: &str,
    en_suspens: &mut Vec<(String, (f64, f64))>,
) {
    let Some(premiere) = en_suspens.first().map(|(m, _)| m.clone()) else { return };
    let i = serie(series, version, "Claude non lancé", oreille, &premiere);
    for (m, t) in en_suspens.drain(..) {
        series[i].transcriptions.push(t);
        series[i].au = series[i].au.clone().max(m);
    }
}

/// La série d'une version, d'un réglage de Claude et d'une oreille (créée au besoin), qui s'étend jusqu'à cette minute.
fn serie(series: &mut Vec<Serie>, version: &str, claude: &str, oreille: &str, minute: &str) -> usize {
    let i = match series.iter().position(|s| s.version == version && s.claude == claude && s.oreille == oreille) {
        Some(i) => i,
        None => {
            series.push(Serie {
                version: version.to_string(),
                claude: claude.to_string(),
                oreille: oreille.to_string(),
                du: minute.to_string(),
                ..Serie::default()
            });
            series.len() - 1
        }
    };
    let s = &mut series[i];
    if s.au.as_str() < minute {
        s.au = minute.to_string();
    }
    i
}

/// Une ligne du journal : sa minute (« 2026-10-23 12:19 ») et son message ; `None` pour une ligne sans heure (la
/// suite d'un message de plusieurs lignes).
fn horodatee(ligne: &str) -> Option<(&str, &str)> {
    let tete = ligne.get(..20)?.as_bytes();
    let chiffres = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    let forme = chiffres.iter().all(|&i| tete[i].is_ascii_digit())
        && tete[4] == b'-'
        && tete[7] == b'-'
        && tete[10] == b' '
        && tete[13] == b':'
        && tete[16] == b':'
        && tete[19] == b' ';
    forme.then(|| (&ligne[..16], &ligne[20..]))
}

/// « Eve 0.1.0 (a1b2c3d) démarre (Sam). » : la version et l'empreinte. Pas « Eve démarrera avec Windows », ni
/// « Eve ne démarre plus… » : une version commence par un chiffre.
fn demarrage(message: &str) -> Option<&str> {
    let reste = message.strip_prefix("Eve ")?;
    let (version, _) = reste.split_once(" démarre (")?;
    version.starts_with(|c: char| c.is_ascii_digit()).then_some(version)
}

/// « entendu (1.8 s de parole, 4.3 s) : … » : les secondes de parole, puis celles du calcul.
fn transcription(message: &str) -> Option<(f64, f64)> {
    let (dedans, _) = message.strip_prefix("entendu (")?.split_once(") : ")?;
    let (parole, calcul) = dedans.split_once(" s de parole, ")?;
    let calcul = calcul.strip_suffix(" s")?;
    Some((parole.parse().ok()?, calcul.parse().ok()?))
}

/// « fin du tour (10.1 s, 0.119 $ ; cumul 0.292 $) : … » : la durée, le coût, et s'il est celui du tour (`true`) ou,
/// dans un journal d'avant, le cumul de la séance (« fin du tour (10.1 s, 0.292 $) »).
fn fin_du_tour(message: &str) -> Option<(Option<f64>, Option<f64>, bool)> {
    let (dedans, _) = message.strip_prefix("fin du tour (")?.split_once(") : ")?;
    let (dedans, du_tour) = match dedans.split_once(" ; cumul ") {
        Some((avant, _)) => (avant, true),
        None => (dedans, false),
    };
    let (duree, cout) = dedans.split_once(", ").unwrap_or((dedans, ""));
    let duree = duree.strip_suffix(" s").and_then(|d| d.parse().ok());
    let cout = cout.strip_suffix(" $").and_then(|c| c.parse().ok());
    Some((duree, cout, du_tour))
}

/// Le relevé des séries pour la console : par étape, le nombre de mesures, la médiane, le 90e centile et le pire.
pub fn releve(series: &[Serie]) -> String {
    let mut s = String::new();
    for serie in series {
        let oreille = if serie.oreille.is_empty() { String::new() } else { format!(" · {}", serie.oreille) };
        let _ =
            writeln!(s, "\nVersion {} · {}{oreille} · du {} au {}", serie.version, serie.claude, serie.du, serie.au);
        let _ = writeln!(s, "  {:<22}{:>6}{:>11}{:>11}{:>11}", "étape", "n", "médiane", "p90", "pire");
        let paroles: Vec<f64> = serie.transcriptions.iter().map(|t| t.0).collect();
        let calculs: Vec<f64> = serie.transcriptions.iter().map(|t| t.1).collect();
        if let Some(l) = rangee("transcription", &calculs, " s") {
            let parole =
                mediane(&paroles).map(|p| format!("   pour {} de parole", nombre(p, " s"))).unwrap_or_default();
            let _ = writeln!(s, "{l}{parole}");
        }
        // Une version qui porte son empreinte note les mesures : sans elles, c'est qu'aucune demande n'est venue.
        if serie.demandes.is_empty() && serie.version.contains('(') {
            let _ = writeln!(s, "  (pas encore de demande mesurée)");
        } else if serie.demandes.is_empty() {
            let _ = writeln!(s, "  (sans « mesures du tour » : ce programme ne les notait pas encore)");
        }
        for e in Etape::TOUTES {
            let v: Vec<f64> = serie.demandes.iter().filter_map(|t| t.de(e)).collect();
            if let Some(l) = rangee(e.nom(), &v, " s") {
                let _ = writeln!(s, "{l}");
            }
        }
        let durees: Vec<f64> = serie.tours.iter().filter_map(|t| t.0).collect();
        let couts: Vec<f64> = serie.tours.iter().filter_map(|t| t.1).collect();
        for (nom, v, unite) in [("tour de Claude", &durees, " s"), ("coût d'une demande", &couts, " $")] {
            if let Some(l) = rangee(nom, v, unite) {
                let _ = writeln!(s, "{l}");
            }
        }
        let silencieux: f64 = serie.silencieux.iter().flatten().sum();
        if !serie.silencieux.is_empty() {
            let n = serie.silencieux.len();
            let _ = writeln!(
                s,
                "  tours silencieux : {n} (le préchauffage, le contexte gardé au chaud), {} en tout",
                nombre(silencieux, " $")
            );
        }
        let total = couts.iter().sum::<f64>() + silencieux;
        if total > 0.0 {
            let _ = writeln!(s, "  coût total : {}", nombre(total, " $"));
        }
    }
    s
}

/// Une rangée du relevé ; `None` sans valeur.
fn rangee(nom: &str, valeurs: &[f64], unite: &str) -> Option<String> {
    let (m, p, pire) = (mediane(valeurs)?, centile(valeurs, 90.0)?, centile(valeurs, 100.0)?);
    Some(format!(
        "  {nom:<22}{:>6}{:>11}{:>11}{:>11}",
        valeurs.len(),
        nombre(m, unite),
        nombre(p, unite),
        nombre(pire, unite)
    ))
}

/// Un nombre du relevé : les dollars au millième (au dix-millième sous un cent) ; les secondes au centième sous dix
/// secondes, au dixième au-delà.
fn nombre(v: f64, unite: &str) -> String {
    match unite {
        " $" if v < 0.01 => format!("{v:.4}{unite}"),
        " $" => format!("{v:.3}{unite}"),
        _ if v < 10.0 => format!("{v:.2}{unite}"),
        _ => format!("{v:.1}{unite}"),
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn la_ligne_des_mesures_se_relit() {
        let mut t = Temps::default();
        t.poser(Etape::Oreille, 4.314);
        t.poser(Etape::Attente, 0.0);
        t.poser(Etape::PremierMot, 2.9);
        t.poser(Etape::PremiereParole, 8.126);
        let ligne = t.ligne();
        assert_eq!(
            ligne,
            "mesures du tour : oreille 4.31 s, attente 0.00 s, premier mot 2.90 s, première parole 8.13 s"
        );
        let relue = Temps::lire(&ligne).unwrap();
        assert_eq!(relue.de(Etape::Oreille), Some(4.31));
        assert_eq!(relue.de(Etape::PremiereParole), Some(8.13));
        assert_eq!(relue.de(Etape::Voix), None, "une étape absente reste absente");
        assert_eq!(Temps::lire("fin du tour (1.0 s, ) : x"), None);
        // Une étape illisible s'ignore ; le reste compte.
        let t = Temps::lire("mesures du tour : oreille x s, voix 0.50 s, inconnue 3 s").unwrap();
        assert_eq!((t.de(Etape::Oreille), t.de(Etape::Voix)), (None, Some(0.5)));
    }

    #[test]
    fn le_cout_du_tour_est_la_difference_du_cumul() {
        assert_eq!(cout_du_tour(None, 0.173), 0.173, "le premier tour de la séance");
        assert!((cout_du_tour(Some(0.173), 0.292) - 0.119).abs() < 1e-9);
        assert_eq!(cout_du_tour(Some(1.4), 0.17), 0.17, "le cumul a baissé : une séance neuve");
    }

    #[test]
    fn mediane_et_centiles() {
        assert_eq!(mediane(&[]), None);
        assert_eq!(mediane(&[3.0]), Some(3.0));
        assert_eq!(mediane(&[5.0, 1.0, 3.0]), Some(3.0));
        assert_eq!(mediane(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
        let v: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(centile(&v, 90.0), Some(9.0));
        assert_eq!(centile(&v, 100.0), Some(10.0));
        assert_eq!(centile(&[7.0], 90.0), Some(7.0));
        assert_eq!(mediane(&[f64::NAN, 2.0]), Some(2.0), "une valeur qui n'est pas un nombre ne compte pas");
    }

    /// Un journal d'avant les mesures (le coût noté était le cumul), puis un démarrage d'une version qui les note.
    const JOURNAL: &str = "\
2026-10-23 12:19:53 Eve 0.1.0 démarre (Sam).
2026-10-23 12:19:53 Claude Code lancé (claude-sonnet-5-5, effort medium)
2026-10-23 12:20:07 fin du tour (7.3 s, 0.173 $) : Prête.
2026-10-23 12:20:07 préchauffage fait : Claude est prêt
2026-10-23 13:07:20 entendu (1.8 s de parole, 4.3 s) : Salut
2026-10-23 13:07:20 demande : Salut
2026-10-23 13:07:31 fin du tour (10.1 s, 0.292 $) : Salut Sam! (Une parenthèse) : et la suite.
2026-10-23 13:38:35 fin du tour (2.9 s, 0.305 $) : Oui.
2026-10-23 13:40:01 Eve ne démarre plus avec Windows.
2026-10-23 14:00:00 le contexte va refroidir : un tour silencieux le garde au chaud
2026-10-23 14:00:01 fin du tour (1.9 s, 0.322 $) : Prête.
2026-10-23 14:00:01 préchauffage fait : Claude est prêt
une suite de message, sans heure
2026-10-23 14:23:50 Eve 0.1.0 (a1b2c3d) démarre (Sam).
2026-10-23 14:23:59 entendu (2.0 s de parole, 3.0 s) : avant le lancement
2026-10-23 14:24:16 Claude Code lancé (claude-opus-5-5[1m], effort max, la conversation reprise)
2026-10-23 14:24:25 fin du tour (1.6 s, 0.171 $ ; cumul 0.171 $) : Prête.
2026-10-23 14:24:25 la base s'est branchée pendant le préchauffage : un second tour silencieux suit
2026-10-23 14:28:53 entendu (3.0 s de parole, 5.0 s) : Eve, le fichier
2026-10-23 14:29:30 fin du tour (37.0 s, 0.214 $ ; cumul 0.385 $) : Je l'ai allégé.
2026-10-23 14:29:31 mesures du tour : oreille 5.10 s, attente 0.00 s, premier mot 3.20 s, première phrase 0.40 s, voix 0.50 s, première parole 9.20 s
2026-10-23 14:30:10 fin du tour (1.7 s, ) : Parfait.
2026-10-23 14:30:10 mesures du tour : attente 0.00 s, premier mot 1.20 s, première phrase 0.30 s
";

    #[test]
    fn un_journal_se_relit_par_version_et_par_reglage() {
        let series = lire_journal(JOURNAL);
        assert_eq!(series.len(), 2);
        let (a, b) = (&series[0], &series[1]);
        assert_eq!((a.version.as_str(), a.claude.as_str()), ("0.1.0", "claude-sonnet-5-5, effort medium"));
        assert_eq!((a.du.as_str(), a.au.as_str()), ("2026-10-23 12:20", "2026-10-23 14:00"));
        assert_eq!(a.transcriptions, vec![(1.8, 4.3)]);
        // Le cumul devient le coût de chaque tour ; le préchauffage et le tour au chaud sont silencieux.
        let millieme = |c: &Option<f64>| (c.unwrap() * 1000.0).round() / 1000.0;
        assert_eq!(a.tours.iter().map(|t| millieme(&t.1)).collect::<Vec<_>>(), vec![0.119, 0.013]);
        assert_eq!(a.tours[0].0, Some(10.1));
        assert_eq!(a.silencieux.iter().map(millieme).collect::<Vec<_>>(), vec![0.173, 0.017]);
        assert!(a.demandes.is_empty());

        assert_eq!((b.version.as_str(), b.claude.as_str()), ("0.1.0 (a1b2c3d)", "claude-opus-5-5[1m], effort max"));
        assert_eq!(b.du, "2026-10-23 14:23", "la transcription d'avant le lancement compte dans la série");
        assert_eq!(b.transcriptions, vec![(2.0, 3.0), (3.0, 5.0)]);
        assert_eq!(b.silencieux, vec![Some(0.171)]);
        assert_eq!(b.tours, vec![(Some(37.0), Some(0.214)), (Some(1.7), None)]);
        assert_eq!(b.demandes.len(), 2);
        assert_eq!(b.demandes[0].de(Etape::PremiereParole), Some(9.2));
        assert_eq!(b.demandes[1].de(Etape::Oreille), None, "une demande écrite n'a pas d'oreille");
    }

    /// Les mots d'une rangée du relevé, après son nom (la première rangée de ce nom).
    fn rangee_de<'a>(releve: &'a str, nom: &str) -> Vec<&'a str> {
        let ligne = releve.lines().find(|l| l.starts_with(&format!("  {nom}  "))).unwrap_or_else(|| panic!("{releve}"));
        ligne[2 + nom.len()..].split_whitespace().collect()
    }

    #[test]
    fn le_releve_donne_les_medianes() {
        let r = releve(&lire_journal(JOURNAL));
        let entete = "Version 0.1.0 · claude-sonnet-5-5, effort medium · du 2026-10-23 12:20 au 2026-10-23 14:00";
        assert!(r.contains(entete), "{r}");
        assert!(r.contains("ce programme ne les notait pas encore"), "{r}");
        assert_eq!(rangee_de(&r, "coût d'une demande"), ["2", "0.066", "$", "0.119", "$", "0.119", "$"]);
        assert_eq!(
            rangee_de(&r, "transcription"),
            ["1", "4.30", "s", "4.30", "s", "4.30", "s", "pour", "1.80", "s", "de", "parole"]
        );
        assert!(r.contains("tours silencieux : 2"), "{r}");
        assert!(r.contains("coût total : 0.322 $"), "{r}");
        // La seconde série : les mesures des demandes.
        assert_eq!(rangee_de(&r, "première parole"), ["1", "9.20", "s", "9.20", "s", "9.20", "s"]);
        assert_eq!(rangee_de(&r, "premier mot"), ["2", "2.20", "s", "3.20", "s", "3.20", "s"]);
        assert_eq!(rangee_de(&r, "tour de Claude")[..2], ["2", "6.50"], "la première série d'abord : {r}");
        assert!(r.contains("coût total : 0.385 $"), "{r}");
        assert_eq!(nombre(0.0004, " $"), "0.0004 $", "un tour de Haiku ne vaut pas zéro");
    }

    /// La même version, le même réglage de Claude : l'oreille passée du processeur à la carte fait une autre série.
    #[test]
    fn l_oreille_fait_sa_serie() {
        let journal = "\
2026-10-23 17:12:54 Eve 0.1.0 (7a487d8) démarre (Sam).
2026-10-23 17:12:56 Claude Code lancé (claude-sonnet-5-5, effort medium)
2026-10-23 17:12:58 oreille : Whisper small sur le processeur, chargé en 3.2 s (avec le rapide)
2026-10-23 17:13:20 entendu (4.4 s de parole, 7.6 s) : une demande
2026-10-23 17:20:00 Eve 0.1.0 (7a487d8) démarre (Sam).
2026-10-23 17:20:01 Claude Code lancé (claude-sonnet-5-5, effort medium)
2026-10-23 17:20:03 oreille : Whisper small sur la carte graphique (port 8178, prêt en 1.8 s, échauffé en 0.7 s)
2026-10-23 17:20:30 entendu (4.4 s de parole, 0.5 s) : une demande
";
        let series = lire_journal(journal);
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].oreille, "Whisper small sur le processeur");
        assert_eq!(series[0].transcriptions, vec![(4.4, 7.6)]);
        assert_eq!(series[1].oreille, "Whisper small sur la carte graphique");
        assert_eq!(series[1].transcriptions, vec![(4.4, 0.5)]);
        let r = releve(&series);
        assert!(r.contains("effort medium · Whisper small sur la carte graphique · du 2026-10-23 17:20 au"), "{r}");
        assert!(r.contains("(pas encore de demande mesurée)"), "une version qui mesure, sans demande : {r}");
        assert!(!r.contains("ne les notait pas encore"), "{r}");
    }

    #[test]
    fn rien_a_relire() {
        assert!(lire_journal("").is_empty());
        assert!(lire_journal("pas un journal\n\n").is_empty());
        let series = lire_journal("2026-10-23 08:00:00 entendu (1.0 s de parole, 2.0 s) : allô\n");
        assert_eq!(series.len(), 1);
        assert_eq!(series[0].claude, "Claude non lancé");
        assert_eq!(series[0].transcriptions, vec![(1.0, 2.0)]);
        // Une Eve dont Claude ne s'est pas lancé garde ses transcriptions ; un lancement sans rien après n'en fait pas.
        let series = lire_journal(
            "2026-10-23 08:00:00 Eve 0.1.0 démarre (S).\n2026-10-23 08:00:05 entendu (1.0 s de parole, 2.0 s) : allô\n\
             2026-10-23 09:00:00 Eve 0.2.0 démarre (S).\n2026-10-23 09:00:01 Claude Code lancé (m, effort low)\n",
        );
        assert_eq!(series.len(), 1);
        assert_eq!((series[0].version.as_str(), series[0].claude.as_str()), ("0.1.0", "Claude non lancé"));
    }
}
