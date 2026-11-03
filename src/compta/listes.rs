//! Le calcul des listes de l'onglet « Comptabilité » : les factures des fournisseurs et celles des clients (les vues
//! « Factures » et « Clients » d'Entrepreneur Général, `comptabilite.html` vers 1700-2560), leurs filtres, leur tri et
//! leurs totaux, en centimes. Son dessin : `natif\compta\listes.rs`. Pur : ni egui, ni fichier, ni horloge — le jour
//! arrive en argument.
//!
//! Ce qui compte dans les totaux suit les règles du contrat (`super`) : une dépense ([`Facture::depense`]) du côté des
//! fournisseurs, un revenu ([`Facture::revenu`]) du côté des clients — les sommes d'une liste filtrée sont ainsi celles
//! du tableau de bord qui y mène —, la note de crédit qui se retranche, le reste, le retard. Une facture montrée qui ne
//! compte pas (le brouillon d'un client, une pro forma) se dit à part. Une annulée (« Retirée » dans EG) ne se montre
//! qu'à la demande, à part, après les autres, et ne compte jamais.
//!
//! La recherche, comme celle d'EG : le texte sans accents ni casse (le numéro, celui du fournisseur, le tiers, le
//! chantier, la section), et un montant au cent — le total ou l'avant-taxes ; sans signe, une note de crédit du même
//! montant aussi ; avec un signe, ce signe seul ([`montant_tape`]).

use std::cmp::Ordering;
use std::collections::HashMap;

use eve::parole::normaliser;

use super::{Cents, Chantier, Cote, Facture, Filtres, Periode, Registre, Somme, argent};

// ---- Les filtres de la liste ----------------------------------------------------------------------------------------

/// L'état d'une facture qu'on filtre : à recevoir (d'un client) ou à payer (à un fournisseur), en retard, réglée
/// (encaissée, payée), en brouillon. Les règles sont celles du contrat ([`Facture::reste`], [`Facture::en_retard`],
/// [`Facture::reglee`], [`Facture::brouillon`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Etat {
    AFaire,
    EnRetard,
    Reglees,
    Brouillons,
}

/// Les états, dans l'ordre de leur menu.
pub const ETATS: [Etat; 4] = [Etat::AFaire, Etat::EnRetard, Etat::Reglees, Etat::Brouillons];

impl Etat {
    /// L'état que nomme un « aller à » du tableau de bord (`natif\compta.rs`, `Aller::etat`) ; « a_verifier » est une
    /// fiabilité ([`FiltresDeListe::de_l_aller`]).
    pub fn de_l_aller(etat: &str) -> Option<Etat> {
        match etat {
            "a_recevoir" | "a_payer" => Some(Etat::AFaire),
            "en_retard" => Some(Etat::EnRetard),
            "reglees" => Some(Etat::Reglees),
            "brouillons" => Some(Etat::Brouillons),
            _ => None,
        }
    }

    /// Ce que dit son menu, de chaque côté.
    pub fn mot(self, cote: Cote) -> &'static str {
        match (self, cote) {
            (Etat::AFaire, Cote::Client) => "À recevoir",
            (Etat::AFaire, Cote::Fournisseur) => "À payer",
            (Etat::EnRetard, _) => "En retard",
            (Etat::Reglees, Cote::Client) => "Encaissées",
            (Etat::Reglees, Cote::Fournisseur) => "Payées",
            (Etat::Brouillons, _) => "Brouillons",
        }
    }

    pub fn retient(self, f: &Facture, aujourdhui: &str) -> bool {
        match self {
            Etat::AFaire => f.reste() > 0,
            Etat::EnRetard => f.en_retard(aujourdhui),
            Etat::Reglees => f.reglee(),
            Etat::Brouillons => f.brouillon(),
        }
    }

    fn rang(self) -> usize {
        ETATS.iter().position(|e| *e == self).unwrap_or(0)
    }
}

/// La fiabilité de la saisie d'une facture reçue, telle que l'ERP la garde aujourd'hui : « sur », « a_verifier », ou
/// rien (non jugée). Une autre valeur (une base plus récente) se montre telle quelle, sans être un choix du filtre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fiabilite {
    Sure,
    AVerifier,
    NonJugee,
    Autre,
}

/// Les choix du filtre de la fiabilité, dans l'ordre de son menu.
pub const FIABILITES: [Fiabilite; 3] = [Fiabilite::Sure, Fiabilite::AVerifier, Fiabilite::NonJugee];

impl Fiabilite {
    pub fn de(f: &Facture) -> Fiabilite {
        match f.fiabilite.as_deref().map(normaliser).as_deref() {
            None | Some("") => Fiabilite::NonJugee,
            Some("sur" | "sure") => Fiabilite::Sure,
            Some("a verifier") => Fiabilite::AVerifier,
            Some(_) => Fiabilite::Autre,
        }
    }

    /// Ce que dit la case et le menu (« Autre » : la valeur de l'ERP, que la case montre elle-même).
    pub fn mot(self) -> &'static str {
        match self {
            Fiabilite::Sure => "Sûre",
            Fiabilite::AVerifier => "À vérifier",
            Fiabilite::NonJugee => "Non jugée",
            Fiabilite::Autre => "Autre",
        }
    }

    fn rang(self) -> Option<usize> {
        FIABILITES.iter().position(|x| *x == self)
    }
}

/// Ce que la liste filtre, en plus de la période et du chantier d'en tête ([`Filtres`]) : ce que la personne y choisit,
/// ou ce qu'apporte un « aller à » du tableau de bord (le mois, la section, le tiers, l'état).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct FiltresDeListe {
    /// Le texte tapé, tel quel.
    pub recherche: String,
    /// Un mois, « AAAA-MM ».
    pub mois: Option<String>,
    /// Un corps de métier ; vide : les factures sans section.
    pub section: Option<String>,
    /// Le fournisseur ou le client, nommé.
    pub tiers: Option<String>,
    pub fiabilite: Option<Fiabilite>,
    pub etat: Option<Etat>,
    /// Les annulées, montrées à part, après les autres (jamais comptées).
    pub annulees: bool,
}

/// Un filtre de la liste, tel que sa pastille le nomme (et que son × l'ôte).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pastille {
    Recherche,
    Mois,
    Section,
    Tiers,
    Fiabilite,
    Etat,
}

impl FiltresDeListe {
    /// Les filtres d'un « aller à » du tableau de bord (`Aller` : `mois`, `section`, `tiers`, `etat`).
    pub fn de_l_aller(
        mois: Option<&str>,
        section: Option<&str>,
        tiers: Option<&str>,
        etat: Option<&str>,
    ) -> FiltresDeListe {
        let propre = |s: Option<&str>| s.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        FiltresDeListe {
            mois: propre(mois).filter(|m| annee_mois(m).is_some()),
            // Une section vide demande les factures sans section : elle se garde.
            section: section.map(|s| s.trim().to_string()),
            tiers: propre(tiers),
            fiabilite: (etat == Some("a_verifier")).then_some(Fiabilite::AVerifier),
            etat: etat.and_then(Etat::de_l_aller),
            ..FiltresDeListe::default()
        }
    }

    /// Les filtres actifs, chacun sa pastille (ce qu'on ôte d'un ×) : la recherche, le mois, la section, le tiers, la
    /// fiabilité, l'état.
    pub fn pastilles(&self, cote: Cote) -> Vec<(Pastille, String)> {
        let mut p = Vec::new();
        let recherche = self.recherche.trim();
        // Une recherche qui ne cherche rien (de la ponctuation seule, « + » ou « - » le temps de taper un montant) ne
        // filtre pas : elle ne se dit pas non plus filtre (la QA5 du lot 13.2).
        if cherche_quelque_chose(recherche) {
            p.push((Pastille::Recherche, format!("Recherche : « {recherche} »")));
        }
        if let Some(m) = &self.mois {
            p.push((Pastille::Mois, format!("Mois : {}", nom_du_mois(m))));
        }
        if let Some(s) = &self.section {
            let s = s.trim();
            p.push((Pastille::Section, if s.is_empty() { "Sans section".into() } else { format!("Section : {s}") }));
        }
        if let Some(t) = &self.tiers {
            let qui = if cote == Cote::Fournisseur { "Fournisseur" } else { "Client" };
            p.push((Pastille::Tiers, format!("{qui} : {t}")));
        }
        if let Some(x) = self.fiabilite {
            p.push((Pastille::Fiabilite, format!("Fiabilité : {}", x.mot())));
        }
        if let Some(e) = self.etat {
            p.push((Pastille::Etat, format!("État : {}", e.mot(cote))));
        }
        p
    }

    /// Combien de filtres sont actifs (les annulées montrées n'en sont pas un : elles ne retirent rien).
    pub fn actifs(&self) -> usize {
        self.pastilles(Cote::Client).len()
    }

    /// Ôte un filtre (le × de sa pastille).
    pub fn oter(&mut self, p: Pastille) {
        match p {
            Pastille::Recherche => self.recherche.clear(),
            Pastille::Mois => self.mois = None,
            Pastille::Section => self.section = None,
            Pastille::Tiers => self.tiers = None,
            Pastille::Fiabilite => self.fiabilite = None,
            Pastille::Etat => self.etat = None,
        }
    }

    /// Tous les filtres ôtés ; les annulées restent comme la personne les a laissées.
    pub fn effaces(&self) -> FiltresDeListe {
        FiltresDeListe { annulees: self.annulees, ..FiltresDeListe::default() }
    }
}

/// « AAAA-MM » → ses nombres.
fn annee_mois(m: &str) -> Option<(i32, u8)> {
    let (a, mm) = m.trim().split_once('-')?;
    let a: i32 = a.parse().ok()?;
    let mm: u8 = mm.get(..2)?.parse().ok()?;
    (a > 0 && (1..=12).contains(&mm)).then_some((a, mm))
}

/// « 2026-03 » → « mars 2026 » (tel quel s'il ne se lit pas).
pub fn nom_du_mois(m: &str) -> String {
    annee_mois(m).map_or_else(|| m.to_string(), |(a, mm)| Periode::Mois(a, mm).libelle())
}

// ---- La recherche par montant ---------------------------------------------------------------------------------------

/// Un montant tapé dans la recherche : ses centimes, signés, et s'il porte un signe (« -45 », « +45 » : ce signe
/// seul ; « 45 » : les deux).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Montant {
    pub cents: Cents,
    pub signe: bool,
}

impl Montant {
    /// Une facture porte-t-elle ce montant, au total ou avant taxes ? Ses montants signés (une note de crédit est
    /// négative) ; sans signe tapé, au cent près sans égard au signe.
    pub fn porte_sur(self, f: &Facture) -> bool {
        let egal = |c: Cents| if self.signe { c == self.cents } else { c.abs() == self.cents.abs() };
        (f.total.is_some() && egal(f.total_signe())) || (f.sous_total.is_some() && egal(f.sous_total_signe()))
    }
}

/// Des chiffres groupés par trois, séparés par `sep` (ou d'un seul tenant sans séparateur).
fn groupes_ok(t: &str, sep: Option<char>) -> bool {
    let chiffres = |g: &str| g.bytes().all(|b| b.is_ascii_digit());
    match sep {
        None => chiffres(t),
        Some(s) => {
            let mut g = t.split(s);
            let premier = g.next().unwrap_or("");
            (1..=3).contains(&premier.len()) && chiffres(premier) && g.all(|x| x.len() == 3 && chiffres(x))
        }
    }
}

/// Un montant tapé dans la recherche — « 1149,75 », « 1 149,75 $ », « 1149.75 », « 1,234.56 », « 1.234,56 », « -45 »
/// — comme `lireMontant` d'EG ; `None` : ce n'est pas un montant — « 1,234 » (1 234 $ en anglais, 1,234 $ en
/// français), plus de deux décimales, des milliers mal groupés. Un séparateur en fin (« 12, » : la frappe n'est pas
/// finie) vaut un montant entier.
pub fn montant_tape(q: &str) -> Option<Montant> {
    let espaces = |c: char| c.is_whitespace() || c == '\u{a0}' || c == '\u{202f}';
    let s: String = q.chars().map(|c| if espaces(c) { ' ' } else { c }).collect();
    let s = s.trim().trim_start_matches('$').trim_end_matches('$').trim();
    let (signe, negatif, s) = match s.chars().next() {
        Some('-' | '\u{2212}') => (true, true, &s[s.chars().next().map_or(0, char::len_utf8)..]),
        Some('+') => (true, false, &s[1..]),
        _ => (false, false, s),
    };
    let s = s.trim().trim_start_matches('$').trim();
    // Les espaces répétées valent une seule.
    let s = s.split(' ').filter(|m| !m.is_empty()).collect::<Vec<_>>().join(" ");
    if !s.bytes().any(|b| b.is_ascii_digit()) || !s.bytes().all(|b| b.is_ascii_digit() || b" .,".contains(&b)) {
        return None;
    }
    let (entiers, decimales) = match s.rfind(['.', ',']) {
        Some(i) => {
            let (avant, apres) = (&s[..i], &s[i + 1..]);
            let sep = char::from(s.as_bytes()[i]);
            let autre = if sep == ',' { '.' } else { ',' };
            let n_sep = s.bytes().filter(|b| *b == b'.' || *b == b',').count();
            let chiffres = apres.bytes().all(|b| b.is_ascii_digit());
            if chiffres && apres.len() <= 2 {
                // Le dernier séparateur est la virgule décimale (ou la fin d'une frappe) ; l'autre signe, ou l'espace,
                // peut séparer les milliers.
                if avant.contains(sep) || (avant.contains(' ') && avant.contains(autre)) {
                    return None;
                }
                let mille = if avant.contains(' ') {
                    Some(' ')
                } else if avant.contains(autre) {
                    Some(autre)
                } else {
                    None
                };
                if !groupes_ok(avant, mille) {
                    return None;
                }
                (avant.to_string(), apres.to_string())
            } else if chiffres && apres.len() == 3 && n_sep > 1 && !s.contains(' ') && !s.contains(autre) {
                // « 1,234,567 » : des milliers seulement.
                if !groupes_ok(&s, Some(sep)) {
                    return None;
                }
                (s.clone(), String::new())
            } else {
                // « 1,234 » est ambigu ; trois décimales et plus ne sont pas un montant.
                return None;
            }
        }
        None => {
            if !groupes_ok(&s, s.contains(' ').then_some(' ')) {
                return None;
            }
            (s.clone(), String::new())
        }
    };
    let chiffres: String = entiers.chars().filter(char::is_ascii_digit).collect();
    let chiffres = chiffres.trim_start_matches('0');
    if chiffres.len() > 13 {
        return None;
    }
    let mut cents: Cents = 0;
    for b in chiffres.bytes() {
        cents = cents * 10 + Cents::from(b - b'0');
    }
    let mut d = decimales.bytes().map(|b| Cents::from(b - b'0'));
    cents = cents * 100 + d.next().unwrap_or(0) * 10 + d.next().unwrap_or(0);
    Some(Montant { cents: if negatif { -cents } else { cents }, signe: signe && cents != 0 })
}

// ---- Les colonnes et le tri -----------------------------------------------------------------------------------------

/// Une colonne d'une liste : ce qu'elle montre, et ce sur quoi elle trie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Colonne {
    Date,
    Tiers,
    Numero,
    Chantier,
    Section,
    AvantTaxes,
    Tps,
    Tvq,
    Taxes,
    Total,
    /// Le reste à payer d'une facture reçue (ou « Payée le … »).
    Reste,
    /// Le paiement d'une facture émise : encaissée, à recevoir, en retard, un crédit.
    Paiement,
    Fiabilite,
    Statut,
}

/// Les colonnes des factures des fournisseurs, et celles des clients : ce que l'ERP a aujourd'hui, rien d'inventé.
pub const COLONNES_DES_FOURNISSEURS: [Colonne; 12] = [
    Colonne::Date,
    Colonne::Tiers,
    Colonne::Numero,
    Colonne::Chantier,
    Colonne::Section,
    Colonne::AvantTaxes,
    Colonne::Tps,
    Colonne::Tvq,
    Colonne::Total,
    Colonne::Reste,
    Colonne::Fiabilite,
    Colonne::Statut,
];
pub const COLONNES_DES_CLIENTS: [Colonne; 9] = [
    Colonne::Date,
    Colonne::Numero,
    Colonne::Tiers,
    Colonne::Chantier,
    Colonne::AvantTaxes,
    Colonne::Taxes,
    Colonne::Total,
    Colonne::Paiement,
    Colonne::Statut,
];

impl Colonne {
    /// Le titre de son en-tête.
    pub fn titre(self, cote: Cote) -> &'static str {
        match self {
            Colonne::Date => "Date",
            Colonne::Tiers if cote == Cote::Fournisseur => "Fournisseur",
            Colonne::Tiers => "Client",
            Colonne::Numero => "N°",
            Colonne::Chantier => "Chantier",
            Colonne::Section => "Section",
            Colonne::AvantTaxes => "Avant taxes",
            Colonne::Tps => "TPS",
            Colonne::Tvq => "TVQ",
            Colonne::Taxes => "Taxes",
            Colonne::Total => "Total",
            Colonne::Reste => "Reste à payer",
            Colonne::Paiement => "Paiement",
            Colonne::Fiabilite => "Fiabilité",
            Colonne::Statut => "Statut",
        }
    }

    /// Une colonne de montants : alignée à droite.
    pub fn en_montant(self) -> bool {
        matches!(
            self,
            Colonne::AvantTaxes | Colonne::Tps | Colonne::Tvq | Colonne::Taxes | Colonne::Total | Colonne::Reste
        )
    }

    /// Le sens d'un premier clic : la date et les montants du plus grand au plus petit, comme EG ; le reste de A à Z.
    fn descend_d_office(self) -> bool {
        self == Colonne::Date || self.en_montant()
    }
}

/// Le tri d'une liste : une colonne, et son sens. D'office, la date la plus récente d'abord.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Tri {
    pub colonne: Colonne,
    pub descendant: bool,
}

impl Default for Tri {
    fn default() -> Tri {
        Tri { colonne: Colonne::Date, descendant: true }
    }
}

impl Tri {
    /// Un clic sur l'en-tête d'une colonne : la même s'inverse ; une autre commence dans son sens d'office.
    pub fn clic(self, c: Colonne) -> Tri {
        if c == self.colonne {
            Tri { colonne: c, descendant: !self.descendant }
        } else {
            Tri { colonne: c, descendant: c.descend_d_office() }
        }
    }
}

/// Deux textes pliés dans l'ordre naturel : les chiffres comme des nombres (« ff 9 » avant « ff 10 », « 2 fondation »
/// avant « 10 finition »). Un ordre total : à égalité de nombres (« 007 », « 7 »), les octets départagent.
fn naturel(a: &str, b: &str) -> Ordering {
    let (x, y) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < x.len() && j < y.len() {
        if x[i].is_ascii_digit() && y[j].is_ascii_digit() {
            let fi = i + x[i..].iter().take_while(|c| c.is_ascii_digit()).count();
            let fj = j + y[j..].iter().take_while(|c| c.is_ascii_digit()).count();
            let sans_zeros = |s: &[u8]| -> usize { s.iter().take_while(|c| **c == b'0').count() };
            let (na, nb) = (&x[i + sans_zeros(&x[i..fi])..fi], &y[j + sans_zeros(&y[j..fj])..fj]);
            let o = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
            if o != Ordering::Equal {
                return o;
            }
            (i, j) = (fi, fj);
        } else {
            if x[i] != y[j] {
                return x[i].cmp(&y[j]);
            }
            (i, j) = (i + 1, j + 1);
        }
    }
    (x.len() - i).cmp(&(y.len() - j)).then_with(|| x.cmp(y))
}

/// La clé de tri d'une facture dans une colonne ; `None` : une case vide, toujours en dernier.
#[derive(Clone, Copy, Debug)]
enum Cle<'a> {
    Nombre(i64),
    Texte(&'a str),
    /// Un groupe, puis une date (le paiement : à recevoir, les crédits, les encaissées).
    Groupe(u8, &'a str),
}

fn comparer(a: &Cle<'_>, b: &Cle<'_>) -> Ordering {
    match (a, b) {
        (Cle::Nombre(x), Cle::Nombre(y)) => x.cmp(y),
        (Cle::Texte(x), Cle::Texte(y)) => naturel(x, y),
        (Cle::Groupe(g, x), Cle::Groupe(h, y)) => g.cmp(h).then_with(|| x.cmp(y)),
        // Une colonne donne toujours la même sorte de clé.
        _ => Ordering::Equal,
    }
}

/// L'ordre d'office, qui départage aussi deux factures égales dans la colonne triée : la date la plus récente
/// d'abord (sans date : en dernier), puis l'id le plus grand (la dernière saisie).
fn d_office(a: &Facture, b: &Facture) -> Ordering {
    match (&a.date, &b.date) {
        (Some(x), Some(y)) => y.cmp(x),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then(b.id.cmp(&a.id))
}

fn cle<'a>(f: &'a Facture, pl: &'a Plie, c: Colonne, aujourdhui: &str) -> Option<Cle<'a>> {
    let texte = |s: &'a str| (!s.is_empty()).then_some(Cle::Texte(s));
    match c {
        Colonne::Date => f.date.as_deref().map(Cle::Texte),
        Colonne::Tiers => texte(&pl.tiers),
        Colonne::Numero => texte(&pl.numero),
        Colonne::Chantier => texte(&pl.projet),
        Colonne::Section => texte(&pl.section),
        Colonne::AvantTaxes => f.sous_total.map(|_| Cle::Nombre(f.sous_total_signe())),
        Colonne::Tps => f.tps.map(|_| Cle::Nombre(f.tps_signee())),
        Colonne::Tvq => f.tvq.map(|_| Cle::Nombre(f.tvq_signee())),
        Colonne::Taxes => (f.tps.is_some() || f.tvq.is_some()).then(|| Cle::Nombre(f.tps_signee() + f.tvq_signee())),
        Colonne::Total => f.total.map(|_| Cle::Nombre(f.total_signe())),
        Colonne::Reste => match reste_du_fournisseur(f, aujourdhui) {
            Reste::APayer(c, _) => Some(Cle::Nombre(c)),
            Reste::Payee(_) => Some(Cle::Nombre(0)),
            Reste::Rien => None,
        },
        Colonne::Paiement => {
            let date = f.date.as_deref().unwrap_or("");
            match paiement_du_client(f, aujourdhui) {
                Paiement::ARecevoir(_) | Paiement::EnRetard(..) => Some(Cle::Groupe(0, date)),
                Paiement::Credit => Some(Cle::Groupe(1, date)),
                Paiement::Encaissee(_) | Paiement::Soldee | Paiement::MarqueePayee => {
                    Some(Cle::Groupe(2, f.paiement.as_deref().unwrap_or(date)))
                }
                Paiement::Rien => None,
            }
        }
        Colonne::Fiabilite => match pl.fiabilite {
            Fiabilite::Sure => Some(Cle::Nombre(0)),
            Fiabilite::AVerifier => Some(Cle::Nombre(1)),
            Fiabilite::Autre => Some(Cle::Nombre(2)),
            Fiabilite::NonJugee => None,
        },
        Colonne::Statut => texte(&pl.statut),
    }
}

// ---- Ce qui se lit dans les cases -----------------------------------------------------------------------------------

/// Le numéro qu'une liste montre : celui du fournisseur sur sa facture, sinon celui de l'ERP.
pub fn numero_dit(f: &Facture) -> &str {
    match (f.cote, f.numero_fournisseur.as_deref().map(str::trim)) {
        (Cote::Fournisseur, Some(n)) if !n.is_empty() => n,
        _ => f.numero.trim(),
    }
}

/// Ce qu'est le document quand ce n'est pas une facture ordinaire : « Note de crédit », « Pro forma », « Acompte »…
pub fn sorte_dite(f: &Facture) -> Option<String> {
    if f.est_avoir() {
        return Some("Note de crédit".into());
    }
    if f.est_proforma() {
        return Some("Pro forma".into());
    }
    let t = f.type_document.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("FACTURE") {
        return None;
    }
    Some(crate::planning::titre_de_statut(t))
}

/// Le statut tel qu'il se lit (« Brouillon », « Partiellement payée »), sa sorte devant quand c'en est une autre
/// (« Note de crédit · Envoyée »).
pub fn statut_dit(f: &Facture) -> String {
    let statut = crate::planning::titre_de_statut(&f.statut);
    match sorte_dite(f) {
        Some(s) => format!("{s} · {statut}"),
        None => statut,
    }
}

/// La case « Reste à payer » d'une facture reçue : ce qui reste (et ses jours de retard), ou payée (le jour), ou rien
/// (un brouillon, une note de crédit, une annulée, une pro forma).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reste {
    APayer(Cents, i64),
    Payee(Option<String>),
    Rien,
}

pub fn reste_du_fournisseur(f: &Facture, aujourdhui: &str) -> Reste {
    if f.annulee() || f.est_avoir() || f.brouillon() || f.est_proforma() {
        return Reste::Rien;
    }
    let reste = f.reste();
    if reste > 0 {
        Reste::APayer(reste, f.jours_de_retard(aujourdhui))
    } else if f.reglee() {
        Reste::Payee(f.paiement.clone())
    } else {
        Reste::Rien
    }
}

/// La case « Paiement » d'une facture émise : encaissée (le jour), à recevoir (ce qui reste), en retard (ses jours,
/// et ce qui reste), un crédit (une note de crédit), ou rien (un brouillon, une annulée, une pro forma).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Paiement {
    Encaissee(Option<String>),
    /// Payée sans rien d'encaissé, son solde à zéro : soldée par une note de crédit (l'ERP passe une facture à PAYEE
    /// quand ses notes de crédit la couvrent, son payé à zéro : `_recalculate_origin_solde_du`).
    Soldee,
    /// Payée sans rien d'encaissé, son solde intact (un statut changé à la main) ou inconnu (une base sans la colonne) :
    /// marquée payée, sans paiement inscrit — rien ne dit comment.
    MarqueePayee,
    ARecevoir(Cents),
    EnRetard(i64, Cents),
    Credit,
    Rien,
}

pub fn paiement_du_client(f: &Facture, aujourdhui: &str) -> Paiement {
    if f.annulee() {
        return Paiement::Rien;
    }
    if f.est_avoir() {
        return Paiement::Credit;
    }
    if f.brouillon() || f.est_proforma() {
        return Paiement::Rien;
    }
    let reste = f.reste();
    if reste > 0 {
        match f.jours_de_retard(aujourdhui) {
            0 => Paiement::ARecevoir(reste),
            j => Paiement::EnRetard(j, reste),
        }
    } else if f.reglee() && f.encaisse() == 0 {
        // Payée sans rien d'encaissé, jamais « encaissée » (la QA5 du lot 13.2). Son solde à zéro : soldée par une note
        // de crédit. Son solde intact (un statut passé à PAYEE à la main, que l'ERP permet à l'administrateur) ou
        // inconnu : marquée payée, sans paiement inscrit — jamais une note de crédit qui n'existe pas (la QA5, passe 2).
        if f.solde == Some(0) { Paiement::Soldee } else { Paiement::MarqueePayee }
    } else if f.reglee() {
        Paiement::Encaissee(f.paiement.clone())
    } else {
        Paiement::Rien
    }
}

/// Une recherche qui cherche quelque chose : un texte qui reste une fois plié, ou un montant.
fn cherche_quelque_chose(recherche: &str) -> bool {
    !normaliser(recherche).is_empty() || montant_tape(recherche).is_some()
}

/// Ce qu'une facture émise a rapporté jusqu'ici : la règle du contrat ([`Facture::encaisse`]), la même que celle du
/// tableau de bord (cette règle-ci, écrite d'abord ici, y est montée à la fusion de l'équipe).
fn encaisse_de(f: &Facture) -> Cents {
    f.encaisse()
}

/// « 2 jours », « 1 jour ».
fn jours(n: i64) -> String {
    if n > 1 { format!("{n} jours") } else { format!("{n} jour") }
}

/// Où en est une facture, dit en clair : « reste à payer 574,88 $, en retard de 2 jours », « payée le 2026-09-25 »,
/// « à recevoir … », « encaissée le … », « en brouillon », « annulée » ; vide sinon.
fn situation(f: &Facture, aujourdhui: &str) -> String {
    if f.annulee() {
        return "annulée".into();
    }
    if f.brouillon() {
        return "en brouillon".into();
    }
    let le = |d: &Option<String>| d.as_deref().map_or(String::new(), |d| format!(" le {d}"));
    match f.cote {
        Cote::Fournisseur => match reste_du_fournisseur(f, aujourdhui) {
            Reste::APayer(c, 0) => format!("reste à payer {}", argent(c)),
            Reste::APayer(c, j) => format!("reste à payer {}, en retard de {}", argent(c), jours(j)),
            Reste::Payee(d) => format!("payée{}", le(&d)),
            Reste::Rien => String::new(),
        },
        Cote::Client => match paiement_du_client(f, aujourdhui) {
            Paiement::ARecevoir(c) => format!("à recevoir {}", argent(c)),
            Paiement::EnRetard(j, c) => format!("à recevoir {}, en retard de {}", argent(c), jours(j)),
            Paiement::Encaissee(d) => format!("encaissée{}", le(&d)),
            Paiement::Soldee => "soldée par une note de crédit".to_string(),
            Paiement::MarqueePayee => "marquée payée, sans paiement inscrit".to_string(),
            Paiement::Credit | Paiement::Rien => String::new(),
        },
    }
}

/// Le document, en tête d'une phrase : « la facture », « la note de crédit », « la pro forma ».
fn document(f: &Facture) -> &'static str {
    if f.est_avoir() {
        "la note de crédit"
    } else if f.est_proforma() {
        "la pro forma"
    } else {
        "la facture"
    }
}

/// Le total, dit : « total 1 149,75 $ » (une note de crédit, négatif), « sans total ».
fn total_dit(f: &Facture) -> String {
    if f.total.is_some() { format!("total {}", argent(f.total_signe())) } else { "sans total".into() }
}

/// « En parler à Eve » d'une facture : une phrase qui la nomme — son numéro, son tiers, son id dans l'ERP (pour que
/// Claude la retrouve), sa date, son chantier, son total et ce qui en reste. Comme une demande écrite à Eve.
pub fn parler_de(f: &Facture, aujourdhui: &str) -> String {
    let numero = numero_dit(f);
    let numero = if numero.is_empty() { "sans numéro".to_string() } else { numero.to_string() };
    let tiers = f.tiers.trim();
    let qui = match (f.cote, tiers.is_empty()) {
        (_, true) => String::new(),
        (Cote::Fournisseur, false) => format!(" du fournisseur {tiers}"),
        (Cote::Client, false) => format!(" au client {tiers}"),
    };
    // Le numéro de l'ERP quand la liste en montre un autre (celui du fournisseur), et toujours l'id.
    let erp = f.numero.trim();
    let dans_l_erp = if !erp.is_empty() && erp != numero {
        format!("{erp} dans Constructo AI, id {}", f.id)
    } else {
        format!("id {} dans Constructo AI", f.id)
    };
    let date = f.date.as_deref().map_or(String::new(), |d| format!(", du {d}"));
    let chantier = f.projet.as_deref().map(str::trim).filter(|p| !p.is_empty());
    let chantier = chantier.map_or(String::new(), |p| format!(", chantier {p}"));
    let ou = situation(f, aujourdhui);
    let ou = if ou.is_empty() { String::new() } else { format!(", {ou}") };
    format!("Parlons de {} {numero}{qui} ({dans_l_erp}){date}{chantier} : {}{ou}.", document(f), total_dit(f))
}

/// « Me le rappeler demain » d'une facture : ce que dira le rappel.
pub fn rappel_de(f: &Facture, aujourdhui: &str) -> String {
    let numero = numero_dit(f);
    let numero = if numero.is_empty() { "sans numéro" } else { numero };
    let tiers = f.tiers.trim();
    let qui = match (f.cote, tiers.is_empty()) {
        (_, true) => String::new(),
        (Cote::Fournisseur, false) => format!(" de {tiers}"),
        (Cote::Client, false) => format!(" à {tiers}"),
    };
    let ou = situation(f, aujourdhui);
    let ou = if ou.is_empty() { String::new() } else { format!(", {ou}") };
    let doc = document(f);
    let doc = doc.strip_prefix("la ").unwrap_or(doc);
    let mut c = doc.chars();
    let doc: String = c.next().map(|p| p.to_uppercase().chain(c).collect()).unwrap_or_default();
    format!("{doc} {numero}{qui} (id {}) : {}{ou}", f.id, total_dit(f))
}

// ---- Le calcul ------------------------------------------------------------------------------------------------------

/// Ce qu'une facture donne, plié une fois par lecture : son texte pour la recherche (ses champs séparés, pour qu'une
/// recherche ne trouve pas à cheval sur deux), et ses textes de tri.
#[derive(Clone, Debug)]
struct Plie {
    botte: String,
    tiers: String,
    numero: String,
    projet: String,
    section: String,
    statut: String,
    fiabilite: Fiabilite,
}

/// Ce qui se prépare une fois par lecture et par côté : les factures du côté, pliées. La fenêtre le garde tant que la
/// lecture ne change pas ; la recherche ne replie alors plus rien à chaque frappe.
#[derive(Clone, Debug)]
pub struct Preparation {
    cote: Cote,
    /// Les rangs, dans le registre, des factures de ce côté.
    rangs: Vec<usize>,
    plies: Vec<Plie>,
}

impl Preparation {
    pub fn de(r: &Registre, cote: Cote) -> Preparation {
        let mut rangs = Vec::new();
        let mut plies = Vec::new();
        for (i, f) in r.factures.iter().enumerate().filter(|(_, f)| f.cote == cote) {
            let champs = [
                Some(f.numero.as_str()),
                f.numero_fournisseur.as_deref(),
                Some(f.tiers.as_str()),
                f.projet.as_deref(),
                f.section.as_deref(),
            ];
            let botte = champs.into_iter().flatten().map(normaliser).collect::<Vec<_>>().join("\n");
            rangs.push(i);
            plies.push(Plie {
                botte,
                tiers: normaliser(&f.tiers),
                numero: normaliser(numero_dit(f)),
                projet: f.projet.as_deref().map(normaliser).unwrap_or_default(),
                section: f.section.as_deref().map(normaliser).unwrap_or_default(),
                statut: normaliser(&statut_dit(f)),
                fiabilite: Fiabilite::de(f),
            });
        }
        Preparation { cote, rangs, plies }
    }

    pub fn cote(&self) -> Cote {
        self.cote
    }
}

/// Les sommes d'une liste, en centimes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totaux {
    /// Les factures qui comptent : des dépenses (fournisseurs), des revenus (clients) — la note de crédit retranchée.
    pub comptees: Somme,
    /// Montrées, mais pas des dépenses ni des revenus : les brouillons d'un client, les pro forma.
    pub non_comptees: Somme,
    pub brouillons_non_comptes: u32,
    pub pro_forma: u32,
    /// Les annulées montrées à part : jamais comptées.
    pub annulees: u32,
    /// Ce qui reste à recevoir (clients) ou à payer (fournisseurs), et combien de factures le doivent.
    pub reste: Cents,
    pub n_reste: u32,
    /// Ce qui en est en retard, et combien de factures.
    pub en_retard: Cents,
    pub n_en_retard: u32,
    /// Ce que les clients ont payé ([`encaisse_de`]).
    pub encaisse: Cents,
}

/// Une liste calculée : ce qu'elle montre, dans l'ordre, ses sommes, et ce que comptent les choix de ses filtres.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Liste {
    /// Les factures montrées, dans l'ordre du tri : leurs rangs dans le registre.
    pub lignes: Vec<usize>,
    /// Les annulées qui répondent aux filtres, montrées à part, après les autres (quand on les demande) ; et combien
    /// il y en a, montrées ou non (le bouton les compte).
    pub annulees: Vec<usize>,
    pub n_annulees: usize,
    pub totaux: Totaux,
    /// Les sections des factures de la période et du chantier (le nom de la première vue, dans l'ordre naturel), et
    /// combien chacune en donnerait avec les autres filtres ; celles sans section.
    pub sections: Vec<(String, usize)>,
    pub sans_section: Option<usize>,
    /// Ce que donnerait chaque choix de la fiabilité ([`FIABILITES`]) et de l'état ([`ETATS`]), les autres filtres
    /// gardés.
    pub fiabilites: [usize; 3],
    pub etats: [usize; 4],
    /// Les factures de ce côté dans le registre, et celles de la période et du chantier (annulées comprises).
    pub du_cote: usize,
    pub en_tete: usize,
}

const RECHERCHE: u8 = 1;
const MOIS: u8 = 2;
const SECTION: u8 = 4;
const TIERS: u8 = 8;
const FIABILITE: u8 = 16;
const ETAT: u8 = 32;

/// La liste d'un côté (`p` : ses factures préparées) : la période et le chantier d'en tête, puis les filtres de la
/// liste, le tri, les sommes. `aujourdhui` (« AAAA-MM-JJ ») décide du retard et des « 12 derniers mois ».
pub fn calculer(
    r: &Registre,
    p: &Preparation,
    en_tete: &Filtres,
    filtres: &FiltresDeListe,
    tri: Tri,
    aujourdhui: &str,
) -> Liste {
    let q = normaliser(&filtres.recherche);
    let montant = montant_tape(&filtres.recherche);
    let recherche = cherche_quelque_chose(&filtres.recherche);
    let section = filtres.section.as_deref().map(normaliser);
    let tiers = filtres.tiers.as_deref().map(normaliser);
    let mut l = Liste { du_cote: p.rangs.len(), ..Liste::default() };
    // Les sections vues : la clé pliée, le nom, le compte ; et la place de chaque clé (une base peut en avoir des
    // centaines, écrites à la main : jamais une recherche dans la liste pour chacune des 20 000 factures).
    let mut sections: Vec<(String, String, usize)> = Vec::new();
    let mut place_des_sections: HashMap<&str, usize> = HashMap::new();
    let mut sans_section: Option<usize> = None;
    let (mut montrees, mut annulees) = (Vec::new(), Vec::new());
    for (k, (&i, pl)) in p.rangs.iter().zip(&p.plies).enumerate() {
        let f = &r.factures[i];
        if !en_tete.retient(f, aujourdhui) {
            continue;
        }
        l.en_tete += 1;
        let mut rate = 0u8;
        if recherche && !pl.botte.contains(&q) && !montant.is_some_and(|m| m.porte_sur(f)) {
            rate |= RECHERCHE;
        }
        if filtres.mois.as_deref().is_some_and(|m| f.mois() != Some(m)) {
            rate |= MOIS;
        }
        if section.as_ref().is_some_and(|s| pl.section != *s) {
            rate |= SECTION;
        }
        if tiers.as_ref().is_some_and(|t| pl.tiers != *t) {
            rate |= TIERS;
        }
        if filtres.fiabilite.is_some_and(|x| pl.fiabilite != x) {
            rate |= FIABILITE;
        }
        if filtres.etat.is_some_and(|e| !e.retient(f, aujourdhui)) {
            rate |= ETAT;
        }
        if f.annulee() {
            if rate == 0 {
                l.n_annulees += 1;
                if filtres.annulees {
                    annulees.push(k);
                }
            }
            continue;
        }
        // Ce que donnerait chaque choix d'un filtre, les autres gardés (les menus le disent).
        let compte_section = usize::from((rate & !SECTION) == 0);
        if pl.section.is_empty() {
            *sans_section.get_or_insert(0) += compte_section;
        } else if let Some(&j) = place_des_sections.get(pl.section.as_str()) {
            sections[j].2 += compte_section;
        } else {
            let nom = f.section.as_deref().unwrap_or("").trim().to_string();
            place_des_sections.insert(pl.section.as_str(), sections.len());
            sections.push((pl.section.clone(), nom, compte_section));
        }
        if (rate & !FIABILITE) == 0
            && let Some(x) = pl.fiabilite.rang()
        {
            l.fiabilites[x] += 1;
        }
        if (rate & !ETAT) == 0 {
            for e in ETATS.iter().filter(|e| e.retient(f, aujourdhui)) {
                l.etats[e.rang()] += 1;
            }
        }
        if rate != 0 {
            continue;
        }
        montrees.push(k);
        let t = &mut l.totaux;
        let compte = match p.cote {
            Cote::Fournisseur => f.depense(),
            Cote::Client => f.revenu(),
        };
        if compte {
            t.comptees.ajouter(f);
        } else {
            t.non_comptees.ajouter(f);
            if f.est_proforma() {
                t.pro_forma += 1;
            } else if f.brouillon() {
                t.brouillons_non_comptes += 1;
            }
        }
        let reste = f.reste();
        if reste > 0 {
            t.reste += reste;
            t.n_reste += 1;
            if f.en_retard(aujourdhui) {
                t.en_retard += reste;
                t.n_en_retard += 1;
            }
        }
        if p.cote == Cote::Client {
            t.encaisse += encaisse_de(f);
        }
    }
    l.totaux.annulees = annulees.len() as u32;
    l.lignes = trier(r, p, montrees, tri, aujourdhui);
    l.annulees = trier(r, p, annulees, tri, aujourdhui);
    sections.sort_by(|a, b| naturel(&a.0, &b.0));
    l.sections = sections.into_iter().map(|(_, nom, n)| (nom, n)).collect();
    l.sans_section = sans_section;
    l
}

/// Trie des factures préparées (leurs places dans `p`) ; rend leurs rangs dans le registre.
fn trier(r: &Registre, p: &Preparation, places: Vec<usize>, tri: Tri, aujourdhui: &str) -> Vec<usize> {
    let facture = |k: usize| &r.factures[p.rangs[k]];
    let mut cles: Vec<(Option<Cle<'_>>, usize)> =
        places.into_iter().map(|k| (cle(facture(k), &p.plies[k], tri.colonne, aujourdhui), k)).collect();
    cles.sort_by(|(a, ka), (b, kb)| match (a, b) {
        (Some(x), Some(y)) => {
            let o = comparer(x, y);
            (if tri.descendant { o.reverse() } else { o }).then_with(|| d_office(facture(*ka), facture(*kb)))
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => d_office(facture(*ka), facture(*kb)),
    });
    cles.into_iter().map(|(_, k)| p.rangs[k]).collect()
}

// ---- Une liste vide -------------------------------------------------------------------------------------------------

/// Les filtres d'en tête qui réduisent la liste, en clair : la période, le chantier.
fn en_tete_en_clair(r: &Registre, en_tete: &Filtres) -> Vec<String> {
    let mut d = Vec::new();
    if en_tete.periode != Periode::Tout {
        d.push(format!("Période : {}", en_tete.periode.libelle()));
    }
    match en_tete.chantier {
        Chantier::Tous => {}
        Chantier::Aucun => d.push("Sans chantier".into()),
        Chantier::Projet(id) => {
            let nom = r.factures.iter().find(|f| f.projet_id == Some(id)).and_then(|f| f.projet.clone());
            d.push(format!("Chantier : {}", nom.unwrap_or_else(|| format!("projet {id}"))));
        }
    }
    d
}

/// Pourquoi une liste est vide, dit pour la personne : aucune facture de ce côté dans l'ERP ; ou ce qui n'en laisse
/// aucune — la recherche (et le montant qu'elle porte), les autres filtres en clair ; et les annulées qui y
/// répondraient, cachées.
pub fn pourquoi_vide(r: &Registre, cote: Cote, en_tete: &Filtres, filtres: &FiltresDeListe, l: &Liste) -> String {
    let de_qui = if cote == Cote::Fournisseur { "de fournisseur" } else { "de client" };
    if l.du_cote == 0 {
        return format!("Constructo AI n'a encore aucune facture {de_qui}.");
    }
    let mut autres = en_tete_en_clair(r, en_tete);
    autres
        .extend(filtres.pastilles(cote).into_iter().filter(|(p, _)| *p != Pastille::Recherche).map(|(_, texte)| texte));
    let q = filtres.recherche.trim();
    let mut phrases = Vec::new();
    if !q.is_empty() {
        let montant = if montant_tape(q).is_some() { " ni ne porte ce montant" } else { "" };
        phrases.push(format!("Rien ne contient « {q} »{montant}."));
        if !autres.is_empty() {
            phrases.push(format!("Les autres filtres : {}.", autres.join(" · ")));
        }
    } else if !autres.is_empty() {
        phrases.push(format!("Les filtres n'en laissent aucune : {}.", autres.join(" · ")));
    }
    if l.n_annulees > 0 && !filtres.annulees {
        phrases.push(if l.n_annulees > 1 {
            format!(
                "{} factures annulées y répondent : « Annulées » les montre, à part.",
                super::par_milliers(l.n_annulees as u64)
            )
        } else {
            "Une facture annulée y répond : « Annulées » la montre, à part.".into()
        });
    }
    if phrases.is_empty() {
        phrases.push(format!("Aucune facture {de_qui} ici."));
    }
    phrases.join(" ")
}

#[cfg(test)]
mod essais {
    use super::*;

    const AUJOURDHUI: &str = "2026-11-02";

    /// Une facture d'essai : ses montants en centimes, écrits à la main.
    #[allow(clippy::too_many_arguments)]
    fn facture(
        id: i64,
        cote: Cote,
        numero: &str,
        tiers: &str,
        statut: &str,
        date: &str,
        montants: Option<(Cents, Cents, Cents, Cents)>,
        paye: Cents,
    ) -> Facture {
        let (sous_total, tps, tvq, total) = match montants {
            Some((s, a, b, t)) => (Some(s), Some(a), Some(b), Some(t)),
            None => (None, None, None, None),
        };
        Facture {
            id,
            numero: numero.into(),
            cote,
            type_document: "FACTURE".into(),
            statut: statut.into(),
            date: Some(date.into()),
            tiers: tiers.into(),
            sous_total,
            tps,
            tvq,
            total,
            paye: Some(paye),
            solde: total.map(|t| t - paye),
            ..Facture::default()
        }
    }

    fn chantier(mut f: Facture, id: i64, nom: &str) -> Facture {
        f.projet_id = Some(id);
        f.projet = Some(nom.into());
        f
    }

    /// Le registre des essais : sept factures reçues, huit émises. Chaque valeur attendue plus bas est calculée à la
    /// main d'après ces montants.
    fn registre() -> Registre {
        use Cote::{Client, Fournisseur};
        let mut f = Vec::new();
        // Reçues.
        let mut x = facture(
            1,
            Fournisseur,
            "FF-1",
            "Béton Fictif ltée",
            "ENVOYEE",
            "2026-10-01",
            Some((100_000, 5_000, 9_975, 114_975)),
            0,
        );
        x.numero_fournisseur = Some("BE-4021".into());
        x.section = Some("2. Fondation".into());
        x.fiabilite = Some("sur".into());
        x.echeance = Some("2026-10-31".into());
        f.push(chantier(x, 131, "Entrepôt Lévis"));
        let mut x = facture(
            2,
            Fournisseur,
            "FF-2",
            "Toitures Exemple",
            "PARTIELLEMENT_PAYEE",
            "2026-10-20",
            Some((200_000, 10_000, 19_950, 229_950)),
            100_000,
        );
        x.section = Some("4. Toiture".into());
        x.fiabilite = Some("a_verifier".into());
        x.echeance = Some("2026-11-19".into());
        f.push(chantier(x, 133, "École Saint-Romuald"));
        let mut x = facture(
            3,
            Fournisseur,
            "FF-3",
            "Bois Modèle inc.",
            "PAYEE",
            "2026-09-05",
            Some((50_000, 2_500, 4_988, 57_488)),
            57_488,
        );
        x.numero_fournisseur = Some("BR-77".into());
        x.section = Some("3. Structure".into());
        x.paiement = Some("2026-09-25".into());
        f.push(chantier(x, 131, "Entrepôt Lévis"));
        let mut x = facture(
            4,
            Fournisseur,
            "FF-4",
            "Gypse Démo",
            "BROUILLON",
            "2026-10-28",
            Some((80_000, 4_000, 7_980, 91_980)),
            0,
        );
        x.section = Some("10. Finition intérieure".into());
        f.push(chantier(x, 133, "École Saint-Romuald"));
        let x = facture(
            5,
            Fournisseur,
            "FF-5",
            "Gypse Démo",
            "ANNULEE",
            "2026-10-10",
            Some((30_000, 1_500, 2_993, 34_493)),
            0,
        );
        f.push(chantier(x, 133, "École Saint-Romuald"));
        let mut x = facture(
            6,
            Fournisseur,
            "FF-6",
            "Béton Fictif ltée",
            "ENVOYEE",
            "2026-10-15",
            Some((3_914, 196, 390, 4_500)),
            0,
        );
        x.type_document = "AVOIR".into();
        x.section = Some("2. Fondation".into());
        f.push(chantier(x, 131, "Entrepôt Lévis"));
        f.push(facture(7, Fournisseur, "FF-7", "Quincaillerie Modèle", "BROUILLON", "2026-10-30", None, 0));
        // Émises.
        let mut x = facture(
            8,
            Client,
            "FACT-2026-00061",
            "Groupe Fictif inc.",
            "ENVOYEE",
            "2026-09-25",
            Some((1_000_000, 50_000, 99_750, 1_149_750)),
            0,
        );
        x.echeance = Some("2026-10-25".into());
        f.push(chantier(x, 131, "Entrepôt Lévis"));
        let mut x = facture(
            9,
            Client,
            "FACT-2026-00062",
            "Centre de services scolaire Modèle",
            "PARTIELLEMENT_PAYEE",
            "2026-10-25",
            Some((2_000_000, 100_000, 199_500, 2_299_500)),
            919_800,
        );
        x.echeance = Some("2026-11-24".into());
        f.push(chantier(x, 133, "École Saint-Romuald"));
        let mut x = facture(
            10,
            Client,
            "FACT-2026-00058",
            "Groupe Fictif inc.",
            "PAYEE",
            "2026-08-25",
            Some((500_000, 25_000, 49_875, 574_875)),
            574_875,
        );
        x.paiement = Some("2026-09-20".into());
        // Une base sans la colonne du payé : son encaissé est son total moins son solde.
        x.paye = None;
        f.push(chantier(x, 131, "Entrepôt Lévis"));
        let mut x = facture(
            11,
            Client,
            "FACT-2026-00063",
            "Groupe Fictif inc.",
            "ENVOYEE",
            "2026-10-10",
            Some((100_000, 5_000, 9_975, 114_975)),
            // Une note de crédit déjà appliquée : l'ERP la dit payée, et elle n'encaisse rien.
            114_975,
        );
        x.type_document = "AVOIR".into();
        f.push(chantier(x, 131, "Entrepôt Lévis"));
        let x = facture(
            12,
            Client,
            "FACT-2026-00064",
            "Famille Témoin",
            "BROUILLON",
            "2026-10-30",
            Some((640_000, 32_000, 63_840, 735_840)),
            0,
        );
        f.push(chantier(x, 137, "Résidence Bouchard"));
        f.push(facture(
            13,
            Client,
            "FACT-2026-00065",
            "Ville d'Essai",
            "ANNULEE",
            "2026-10-05",
            Some((100_000, 5_000, 9_975, 114_975)),
            0,
        ));
        let mut x = facture(
            14,
            Client,
            "PF-2026-001",
            "Ville d'Essai",
            "ENVOYEE",
            "2026-10-12",
            Some((1_250_000, 62_500, 124_688, 1_437_188)),
            0,
        );
        x.type_document = "PROFORMA".into();
        f.push(chantier(x, 139, "Garage municipal"));
        let mut x = facture(
            15,
            Client,
            "FACT-2026-00066",
            "Habitations Exemple",
            "ENVOYEE",
            "2026-10-30",
            Some((100_000, 5_000, 9_975, 114_975)),
            0,
        );
        x.echeance = Some("2026-11-29".into());
        f.push(chantier(x, 135, "Condos du Fleuve"));
        Registre { au_serveur: f.len() as u64, factures: f, ..Registre::default() }
    }

    /// Les id des factures d'une liste, dans son ordre.
    fn ids(r: &Registre, rangs: &[usize]) -> Vec<i64> {
        rangs.iter().map(|&i| r.factures[i].id).collect()
    }

    fn liste_de(r: &Registre, cote: Cote, en_tete: Filtres, filtres: &FiltresDeListe, tri: Tri) -> Liste {
        calculer(r, &Preparation::de(r, cote), &en_tete, filtres, tri, AUJOURDHUI)
    }

    /// Les id d'une liste filtrée (la période et le chantier : tous ; le tri d'office).
    fn trouve(cote: Cote, filtres: FiltresDeListe) -> Vec<i64> {
        let r = registre();
        let l = liste_de(&r, cote, Filtres::default(), &filtres, Tri::default());
        ids(&r, &l.lignes)
    }

    fn cherche(cote: Cote, q: &str) -> Vec<i64> {
        trouve(cote, FiltresDeListe { recherche: q.into(), ..FiltresDeListe::default() })
    }

    #[test]
    fn la_recherche_en_texte_sans_accents_ni_casse() {
        use Cote::{Client, Fournisseur};
        // L'ordre d'office : la plus récente d'abord (la note de crédit du 15 octobre, puis la facture du 1er).
        for q in ["beton", "BÉTON", "Béton fictif", "fictif LTEE", "  béton  "] {
            assert_eq!(cherche(Fournisseur, q), [6, 1], "{q:?}");
        }
        assert_eq!(cherche(Fournisseur, "ecole"), [4, 2], "le chantier « École Saint-Romuald »");
        assert_eq!(cherche(Fournisseur, "toiture"), [2], "la section et le fournisseur");
        assert_eq!(cherche(Fournisseur, "be-4021"), [1], "le numéro du fournisseur");
        assert_eq!(cherche(Fournisseur, "ff 3"), [3], "le numéro de l'ERP, sa ponctuation pliée");
        assert_eq!(cherche(Client, "fact-2026-00062"), [9]);
        // Une recherche ne trouve pas à cheval sur deux champs (le fournisseur et son chantier).
        assert_eq!(cherche(Fournisseur, "ltee entrepot"), Vec::<i64>::new());
        assert_eq!(cherche(Fournisseur, "zzz"), Vec::<i64>::new());
    }

    #[test]
    fn la_recherche_par_montant_au_cent_signe_compris() {
        use Cote::{Client, Fournisseur};
        // Sans signe : la facture de 1 149,75 $ et la note de crédit du même montant ; ses quatre écritures.
        for q in ["1149,75", "1 149,75", "1149.75", "1\u{a0}149,75 $", "$1149.75"] {
            assert_eq!(cherche(Client, q), [15, 11], "{q:?}");
        }
        // Avec un signe : celui-là seul.
        assert_eq!(cherche(Client, "-1149,75"), [11]);
        assert_eq!(cherche(Client, "\u{2212}1 149,75"), [11]);
        assert_eq!(cherche(Client, "+1149,75"), [15]);
        assert_eq!(cherche(Fournisseur, "-45"), [6], "la note de crédit de 45,00 $");
        assert_eq!(cherche(Fournisseur, "45"), [6]);
        // L'avant-taxes compte aussi, signé de même.
        assert_eq!(cherche(Fournisseur, "2000"), [2]);
        assert_eq!(cherche(Fournisseur, "-39,14"), [6]);
        assert_eq!(cherche(Fournisseur, "39,14"), [6]);
        // La TPS et la TVQ ne sont pas cherchées : 50,00 $ est la TPS de la facture 1.
        assert_eq!(cherche(Fournisseur, "50"), Vec::<i64>::new());
        // Un montant qui n'est qu'un préfixe ne trouve RIEN.
        for q in ["1149", "1 149", "1149,7", "114", "1149,750"] {
            assert_eq!(cherche(Client, q), Vec::<i64>::new(), "{q:?}");
            assert_eq!(cherche(Fournisseur, q), Vec::<i64>::new(), "{q:?}");
        }
    }

    #[test]
    fn un_montant_tape() {
        let m = |c: Cents, signe: bool| Some(Montant { cents: c, signe });
        assert_eq!(montant_tape("1149,75"), m(114_975, false));
        assert_eq!(montant_tape("1 149,75 $"), m(114_975, false));
        assert_eq!(montant_tape("1\u{202f}149,75\u{a0}$"), m(114_975, false), "l'écriture d'Eve (`argent`)");
        assert_eq!(montant_tape("1149.75"), m(114_975, false));
        assert_eq!(montant_tape("1,234.56"), m(123_456, false));
        assert_eq!(montant_tape("1.234,56"), m(123_456, false));
        assert_eq!(montant_tape("1,234,567"), m(123_456_700, false));
        assert_eq!(montant_tape("-45"), m(-4_500, true));
        assert_eq!(montant_tape("\u{2212}45,00 $"), m(-4_500, true));
        assert_eq!(montant_tape("+7,5"), m(750, true));
        assert_eq!(montant_tape(",5"), m(50, false));
        assert_eq!(montant_tape("12,"), m(1_200, false), "une frappe pas finie");
        assert_eq!(montant_tape("-0"), m(0, false), "zéro n'a pas de signe");
        for q in [
            "", "abc", "1,234", "1.234", "12,345", "1,2345", "12 34", "1 2345,00", "1.234.56", "1,2,3", "-", "$", "1e3",
        ] {
            assert_eq!(montant_tape(q), None, "{q:?}");
        }
    }

    #[test]
    fn chaque_etat_de_chaque_cote() {
        use Cote::{Client, Fournisseur};
        let etat = |cote: Cote, e: Etat| trouve(cote, FiltresDeListe { etat: Some(e), ..FiltresDeListe::default() });
        assert_eq!(etat(Fournisseur, Etat::AFaire), [2, 1]);
        assert_eq!(etat(Fournisseur, Etat::EnRetard), [1], "échue le 31 octobre");
        assert_eq!(etat(Fournisseur, Etat::Reglees), [3]);
        assert_eq!(etat(Fournisseur, Etat::Brouillons), [7, 4]);
        assert_eq!(etat(Client, Etat::AFaire), [15, 9, 8]);
        assert_eq!(etat(Client, Etat::EnRetard), [8], "échue le 25 octobre");
        assert_eq!(etat(Client, Etat::Reglees), [10]);
        assert_eq!(etat(Client, Etat::Brouillons), [12]);
        // La fiabilité.
        let fiab =
            |x: Fiabilite| trouve(Fournisseur, FiltresDeListe { fiabilite: Some(x), ..FiltresDeListe::default() });
        assert_eq!(fiab(Fiabilite::Sure), [1]);
        assert_eq!(fiab(Fiabilite::AVerifier), [2]);
        assert_eq!(fiab(Fiabilite::NonJugee), [7, 4, 6, 3]);
        // Les annulées : jamais montrées d'office ; à part, après les autres, quand on les demande.
        let r = registre();
        let filtres = FiltresDeListe { annulees: true, ..FiltresDeListe::default() };
        let l = liste_de(&r, Fournisseur, Filtres::default(), &filtres, Tri::default());
        assert_eq!((ids(&r, &l.lignes), ids(&r, &l.annulees)), (vec![7, 4, 2, 6, 1, 3], vec![5]));
        let l = liste_de(&r, Fournisseur, Filtres::default(), &FiltresDeListe::default(), Tri::default());
        assert_eq!((l.annulees.len(), l.n_annulees), (0, 1), "cachée, mais comptée pour son bouton");
        // Un état exclut une annulée : elle ne doit rien et n'est pas réglée.
        let filtres = FiltresDeListe { annulees: true, etat: Some(Etat::AFaire), ..FiltresDeListe::default() };
        assert!(liste_de(&r, Client, Filtres::default(), &filtres, Tri::default()).annulees.is_empty());
    }

    #[test]
    fn les_autres_filtres_et_ceux_d_en_tete() {
        use Cote::{Client, Fournisseur};
        let f = FiltresDeListe::default;
        assert_eq!(trouve(Fournisseur, FiltresDeListe { mois: Some("2026-10".into()), ..f() }), [7, 4, 2, 6, 1]);
        assert_eq!(trouve(Fournisseur, FiltresDeListe { section: Some("4. TOITURE".into()), ..f() }), [2]);
        assert_eq!(trouve(Fournisseur, FiltresDeListe { section: Some(String::new()), ..f() }), [7], "sans section");
        assert_eq!(trouve(Fournisseur, FiltresDeListe { tiers: Some("beton fictif LTÉE".into()), ..f() }), [6, 1]);
        let gypse = FiltresDeListe { recherche: "gypse".into(), etat: Some(Etat::Brouillons), ..f() };
        assert_eq!(trouve(Fournisseur, gypse), [4]);
        // La période et le chantier d'en tête.
        let r = registre();
        let en_tete = |periode: Periode, chantier: Chantier| Filtres { periode, chantier };
        let liste = |e: Filtres| ids(&r, &liste_de(&r, Fournisseur, e, &f(), Tri::default()).lignes);
        assert_eq!(liste(en_tete(Periode::Mois(2026, 9), Chantier::Tous)), [3]);
        assert_eq!(liste(en_tete(Periode::Tout, Chantier::Projet(133))), [4, 2]);
        assert_eq!(liste(en_tete(Periode::Tout, Chantier::Aucun)), [7]);
        // Un « aller à » du tableau de bord.
        let a = FiltresDeListe::de_l_aller(Some("2026-10"), Some("2. Fondation"), None, Some("a_recevoir"));
        assert_eq!((a.mois.as_deref(), a.etat, a.fiabilite), (Some("2026-10"), Some(Etat::AFaire), None));
        assert_eq!(trouve(Fournisseur, a), [1]);
        let a = FiltresDeListe::de_l_aller(None, None, Some("Groupe Fictif inc."), Some("reglees"));
        assert_eq!(trouve(Client, a), [10]);
        let a = FiltresDeListe::de_l_aller(Some("mars"), None, None, Some("a_verifier"));
        assert_eq!((a.mois, a.etat, a.fiabilite), (None, None, Some(Fiabilite::AVerifier)));
        // Les menus comptent chaque choix avec les autres filtres : à payer, une facture de fondation, une de toiture.
        let r = registre();
        let filtres = FiltresDeListe { etat: Some(Etat::AFaire), ..f() };
        let l = liste_de(&r, Fournisseur, Filtres::default(), &filtres, Tri::default());
        let sections: Vec<(&str, usize)> = l.sections.iter().map(|(s, n)| (s.as_str(), *n)).collect();
        assert_eq!(
            sections,
            [("2. Fondation", 1), ("3. Structure", 0), ("4. Toiture", 1), ("10. Finition intérieure", 0)]
        );
        assert_eq!((l.sans_section, l.fiabilites), (Some(0), [1, 1, 0]));
        assert_eq!(l.etats, [2, 1, 1, 2], "l'état ne compte pas son propre filtre");
        // La section et la fiabilité ne comptent pas non plus le leur.
        let filtres = FiltresDeListe { section: Some("4. Toiture".into()), fiabilite: Some(Fiabilite::Sure), ..f() };
        let l = liste_de(&r, Fournisseur, Filtres::default(), &filtres, Tri::default());
        assert!(l.lignes.is_empty(), "la toiture n'a pas de facture sûre");
        let sections: Vec<(&str, usize)> = l.sections.iter().map(|(s, n)| (s.as_str(), *n)).collect();
        assert_eq!(
            sections,
            [("2. Fondation", 1), ("3. Structure", 0), ("4. Toiture", 0), ("10. Finition intérieure", 0)]
        );
        assert_eq!((l.sans_section, l.fiabilites), (Some(0), [0, 1, 0]));
    }

    #[test]
    fn le_tri_et_les_cases_vides_en_dernier() {
        use Cote::{Client, Fournisseur};
        let r = registre();
        let ordre = |cote: Cote, colonne: Colonne, descendant: bool| {
            let l = liste_de(&r, cote, Filtres::default(), &FiltresDeListe::default(), Tri { colonne, descendant });
            ids(&r, &l.lignes)
        };
        assert_eq!(ordre(Fournisseur, Colonne::Date, true), [7, 4, 2, 6, 1, 3], "d'office");
        assert_eq!(ordre(Fournisseur, Colonne::Date, false), [3, 1, 6, 2, 4, 7]);
        // Le total signé (la note de crédit, −45,00 $) ; la facture sans total, en dernier dans les deux sens.
        assert_eq!(ordre(Fournisseur, Colonne::Total, true), [2, 1, 4, 3, 6, 7]);
        assert_eq!(ordre(Fournisseur, Colonne::Total, false), [6, 3, 4, 1, 2, 7]);
        // Le chantier : les égaux dans l'ordre d'office ; sans chantier, en dernier dans les deux sens.
        assert_eq!(ordre(Fournisseur, Colonne::Chantier, false), [4, 2, 6, 1, 3, 7]);
        assert_eq!(ordre(Fournisseur, Colonne::Chantier, true), [6, 1, 3, 4, 2, 7]);
        // Les sections dans l'ordre naturel : la 10 après la 4.
        assert_eq!(ordre(Fournisseur, Colonne::Section, false), [6, 1, 3, 2, 4, 7]);
        assert_eq!(ordre(Fournisseur, Colonne::Tiers, false), [6, 1, 3, 4, 7, 2]);
        // Le reste à payer : les plus gros d'abord, puis la payée, puis les cases vides.
        assert_eq!(ordre(Fournisseur, Colonne::Reste, true), [2, 1, 3, 7, 4, 6]);
        // La fiabilité : sûre, à vérifier, puis les non jugées.
        assert_eq!(ordre(Fournisseur, Colonne::Fiabilite, false), [1, 2, 7, 4, 6, 3]);
        // Le paiement : à recevoir (la plus ancienne d'abord), le crédit, l'encaissée ; le brouillon et la pro forma.
        assert_eq!(ordre(Client, Colonne::Paiement, false), [8, 9, 15, 11, 10, 12, 14]);
        assert_eq!(ordre(Client, Colonne::Paiement, true), [10, 11, 15, 9, 8, 12, 14]);
        // Le numéro, dans l'ordre naturel.
        assert_eq!(naturel("ff 9", "ff 10"), Ordering::Less);
        assert_eq!(naturel("ff 010", "ff 9"), Ordering::Greater);
        assert_eq!(naturel("a", "a 1"), Ordering::Less);
        // Un clic : la même colonne s'inverse ; une autre part dans son sens d'office.
        let t = Tri::default();
        assert_eq!(t.clic(Colonne::Date), Tri { colonne: Colonne::Date, descendant: false });
        assert_eq!(t.clic(Colonne::Total), Tri { colonne: Colonne::Total, descendant: true });
        assert_eq!(t.clic(Colonne::Tiers), Tri { colonne: Colonne::Tiers, descendant: false });
        assert_eq!(t.clic(Colonne::Tiers).clic(Colonne::Tiers), Tri { colonne: Colonne::Tiers, descendant: true });
    }

    #[test]
    fn les_totaux_au_cent() {
        use Cote::{Client, Fournisseur};
        let r = registre();
        // Les fournisseurs : six factures comptent (les brouillons sont des dépenses), la note de crédit retranchée,
        // l'annulée jamais, même montrée ; une sans total.
        for annulees in [false, true] {
            let filtres = FiltresDeListe { annulees, ..FiltresDeListe::default() };
            let t = liste_de(&r, Fournisseur, Filtres::default(), &filtres, Tri::default()).totaux;
            let somme = Somme { n: 6, sous_total: 426_086, tps: 21_304, tvq: 42_503, total: 489_893, sans_total: 1 };
            assert_eq!(t.comptees, somme);
            assert_eq!((t.non_comptees, t.annulees), (Somme::default(), u32::from(annulees)));
            assert_eq!((t.reste, t.n_reste, t.en_retard, t.n_en_retard), (244_925, 2, 114_975, 1));
            assert_eq!(t.encaisse, 0, "seuls les clients encaissent");
        }
        // Les clients : quatre revenus (la note de crédit retranchée) ; le brouillon et la pro forma montrés, non
        // comptés ; l'encaissé, ce que les clients ont payé (la note de crédit n'encaisse rien).
        let t = liste_de(&r, Client, Filtres::default(), &FiltresDeListe::default(), Tri::default()).totaux;
        let somme = Somme { n: 5, sous_total: 3_500_000, tps: 175_000, tvq: 349_125, total: 4_024_125, sans_total: 0 };
        assert_eq!(t.comptees, somme);
        assert_eq!((t.non_comptees.n, t.non_comptees.total), (2, 2_173_028));
        assert_eq!((t.brouillons_non_comptes, t.pro_forma, t.annulees), (1, 1, 0));
        assert_eq!(t.encaisse, 1_494_675);
        assert_eq!((t.reste, t.n_reste, t.en_retard, t.n_en_retard), (2_644_425, 3, 1_149_750, 1));
        // Une liste filtrée a les sommes de ce qu'elle montre : la note de crédit seule.
        let filtres = FiltresDeListe { recherche: "-1149,75".into(), ..FiltresDeListe::default() };
        let t = liste_de(&r, Client, Filtres::default(), &filtres, Tri::default()).totaux;
        assert_eq!((t.comptees.n, t.comptees.total, t.comptees.tvq, t.reste), (1, -114_975, -9_975, 0));
    }

    #[test]
    fn ce_que_disent_les_cases_et_les_demandes() {
        let r = registre();
        let f = |id: i64| r.factures.iter().find(|f| f.id == id).unwrap();
        assert_eq!(numero_dit(f(1)), "BE-4021");
        assert_eq!(numero_dit(f(2)), "FF-2", "sans le numéro du fournisseur, celui de l'ERP");
        assert_eq!(statut_dit(f(6)), "Note de crédit · Envoyée");
        assert_eq!(statut_dit(f(2)), "Partiellement payée");
        assert_eq!(reste_du_fournisseur(f(1), AUJOURDHUI), Reste::APayer(114_975, 2));
        assert_eq!(reste_du_fournisseur(f(3), AUJOURDHUI), Reste::Payee(Some("2026-09-25".into())));
        assert_eq!(reste_du_fournisseur(f(4), AUJOURDHUI), Reste::Rien, "un brouillon ne se paie pas encore");
        assert_eq!(paiement_du_client(f(8), AUJOURDHUI), Paiement::EnRetard(8, 1_149_750));
        assert_eq!(paiement_du_client(f(9), AUJOURDHUI), Paiement::ARecevoir(1_379_700));
        assert_eq!(paiement_du_client(f(10), AUJOURDHUI), Paiement::Encaissee(Some("2026-09-20".into())));
        assert_eq!(paiement_du_client(f(11), AUJOURDHUI), Paiement::Credit);
        assert_eq!(paiement_du_client(f(12), AUJOURDHUI), Paiement::Rien);
        assert_eq!(
            parler_de(f(1), AUJOURDHUI),
            "Parlons de la facture BE-4021 du fournisseur Béton Fictif ltée (FF-1 dans Constructo AI, id 1), du \
             2026-10-01, chantier Entrepôt Lévis : total 1\u{202f}149,75\u{a0}$, reste à payer \
             1\u{202f}149,75\u{a0}$, en retard de 2 jours."
        );
        assert_eq!(
            parler_de(f(11), AUJOURDHUI),
            "Parlons de la note de crédit FACT-2026-00063 au client Groupe Fictif inc. (id 11 dans Constructo AI), \
             du 2026-10-10, chantier Entrepôt Lévis : total \u{2212}1\u{202f}149,75\u{a0}$."
        );
        assert_eq!(
            parler_de(f(7), AUJOURDHUI),
            "Parlons de la facture FF-7 du fournisseur Quincaillerie Modèle (id 7 dans Constructo AI), du \
             2026-10-30 : sans total, en brouillon."
        );
        assert_eq!(
            rappel_de(f(8), AUJOURDHUI),
            "Facture FACT-2026-00061 à Groupe Fictif inc. (id 8) : total 11\u{202f}497,50\u{a0}$, à recevoir \
             11\u{202f}497,50\u{a0}$, en retard de 8 jours"
        );
        assert_eq!(
            rappel_de(f(3), AUJOURDHUI),
            "Facture BR-77 de Bois Modèle inc. (id 3) : total 574,88\u{a0}$, payée le 2026-09-25"
        );
    }

    #[test]
    fn vingt_mille_factures() {
        // Une grande entreprise : 20 000 factures reçues de 1 149,75 $, une section sur quatre, la moitié payées.
        let factures: Vec<Facture> = (0..20_000)
            .map(|k| {
                let payee = k % 2 == 0;
                let mut x = facture(
                    k,
                    Cote::Fournisseur,
                    &format!("FF-{k}"),
                    &format!("Fournisseur {}", k % 50),
                    if payee { "PAYEE" } else { "ENVOYEE" },
                    "2026-10-01",
                    Some((100_000, 5_000, 9_975, 114_975)),
                    if payee { 114_975 } else { 0 },
                );
                x.section = Some(format!("{}. Section", k % 4 + 1));
                x
            })
            .collect();
        let r = Registre { au_serveur: 20_000, factures, ..Registre::default() };
        let filtres = FiltresDeListe { recherche: "1 149,75".into(), ..FiltresDeListe::default() };
        let l = liste_de(&r, Cote::Fournisseur, Filtres::default(), &filtres, Tri::default());
        assert_eq!(l.lignes.len(), 20_000);
        assert_eq!((l.totaux.comptees.n, l.totaux.comptees.total), (20_000, 2_299_500_000));
        assert_eq!((l.totaux.reste, l.totaux.n_reste), (1_149_750_000, 10_000));
        assert_eq!(l.sections.iter().map(|s| s.1).collect::<Vec<_>>(), [5_000; 4]);
        // Le tri d'office départage les égaux par l'id : la dernière saisie d'abord.
        assert_eq!((r.factures[l.lignes[0]].id, r.factures[l.lignes[19_999]].id), (19_999, 0));
    }

    /// La QA5 (passe 1) : « + » ou « - » montraient tout, sous une pastille « Recherche ».
    #[test]
    fn une_recherche_qui_ne_cherche_rien_ne_filtre_pas() {
        for s in ["+", " - ", ".", ""] {
            assert!(!cherche_quelque_chose(s), "{s:?}");
        }
        for s in ["-45", "béton", "1 149,75"] {
            assert!(cherche_quelque_chose(s), "{s:?}");
        }
        let f = FiltresDeListe { recherche: "+".into(), ..FiltresDeListe::default() };
        assert!(f.pastilles(Cote::Client).is_empty(), "aucune pastille pour une recherche qui ne cherche rien");
    }

    /// La QA5 (passe 1) : une facture soldée par une note de crédit (PAYEE, rien de payé) se disait « encaissée ».
    #[test]
    fn une_facture_soldee_par_un_credit_ne_se_dit_pas_encaissee() {
        let soldee = Facture {
            id: 1,
            cote: Cote::Client,
            statut: "PAYEE".into(),
            type_document: "FACTURE".into(),
            date: Some("2026-09-01".into()),
            total: Some(10_000),
            paye: Some(0),
            solde: Some(0),
            ..Facture::default()
        };
        assert_eq!(paiement_du_client(&soldee, AUJOURDHUI), Paiement::Soldee);
        assert_eq!(situation(&soldee, AUJOURDHUI), "soldée par une note de crédit");
        // La QA5 (passe 2) : passée à PAYEE à la main, son solde intact, ou une base sans payé ni solde — rien ne dit
        // qu'une note de crédit l'a soldée.
        let marquee = Facture { solde: Some(10_000), ..soldee.clone() };
        assert_eq!(paiement_du_client(&marquee, AUJOURDHUI), Paiement::MarqueePayee);
        assert_eq!(situation(&marquee, AUJOURDHUI), "marquée payée, sans paiement inscrit");
        let ancienne = Facture { paye: None, solde: None, ..soldee.clone() };
        assert_eq!(paiement_du_client(&ancienne, AUJOURDHUI), Paiement::MarqueePayee);
        let payee = Facture { paye: Some(10_000), paiement: Some("2026-09-20".into()), ..soldee };
        assert_eq!(paiement_du_client(&payee, AUJOURDHUI), Paiement::Encaissee(Some("2026-09-20".into())));
    }

    #[test]
    fn une_liste_vide_dit_pourquoi() {
        use Cote::{Client, Fournisseur};
        let r = registre();
        let vide = |cote: Cote, en_tete: Filtres, filtres: FiltresDeListe| {
            let l = liste_de(&r, cote, en_tete, &filtres, Tri::default());
            assert!(l.lignes.is_empty());
            pourquoi_vide(&r, cote, &en_tete, &filtres, &l)
        };
        let f = FiltresDeListe::default;
        assert_eq!(
            vide(Fournisseur, Filtres::default(), FiltresDeListe { recherche: "zzz".into(), ..f() }),
            "Rien ne contient « zzz »."
        );
        let en_tete = Filtres { periode: Periode::Annee(2026), chantier: Chantier::Projet(133) };
        assert_eq!(
            vide(Fournisseur, en_tete, FiltresDeListe { recherche: "12,34".into(), ..f() }),
            "Rien ne contient « 12,34 » ni ne porte ce montant. Les autres filtres : Période : 2026 · Chantier : \
             École Saint-Romuald."
        );
        let filtres = FiltresDeListe { mois: Some("2026-03".into()), etat: Some(Etat::EnRetard), ..f() };
        assert_eq!(
            vide(Client, Filtres::default(), filtres),
            "Les filtres n'en laissent aucune : Mois : mars 2026 · État : En retard."
        );
        // L'annulée qui y répondrait le dit.
        assert_eq!(
            vide(Client, Filtres::default(), FiltresDeListe { recherche: "FACT-2026-00065".into(), ..f() }),
            "Rien ne contient « FACT-2026-00065 ». Une facture annulée y répond : « Annulées » la montre, à part."
        );
        // Aucune facture de ce côté dans l'ERP.
        let seul = Registre { factures: vec![r.factures[0].clone()], ..Registre::default() };
        let l = calculer(&seul, &Preparation::de(&seul, Client), &Filtres::default(), &f(), Tri::default(), AUJOURDHUI);
        assert_eq!(
            pourquoi_vide(&seul, Client, &Filtres::default(), &f(), &l),
            "Constructo AI n'a encore aucune facture de client."
        );
    }
}
