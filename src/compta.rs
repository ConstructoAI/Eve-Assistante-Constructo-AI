//! La comptabilité de l'entreprise (le lot 13.2 : « avec le module de Comptabilité », demandé le 31 octobre ; « Tout de
//! suite », sur la comptabilité ACTUELLE de l'ERP, le 1er novembre) : les vues d'Entrepreneur Général — le tableau de
//! bord, les factures des fournisseurs et celles des clients, les sommaires par mois, par fournisseur et par chantier,
//! l'historique —, d'après Constructo AI lu en lecture seule par deux outils de son serveur MCP :
//! `comptabilite_lister_registre` (les factures, par pages) et `comptabilite_lister_historique` (le journal d'audit de
//! la comptabilité). `entreprise.rs` les appelle ; le chef garde la lecture dans `registre.json` du dossier local
//! ([`Lecture`], [`FICHIER`]) ; le centre la relit ; l'onglet « Comptabilité » du tableau de bord la montre
//! (`natif\compta.rs`).
//!
//! Ce fichier est le CONTRAT de l'équipe qui fait le module :
//! - la forme du registre : ce que rend le serveur ([`Facture::du_serveur`], [`Changement::du_serveur`]), ce que garde
//!   le fichier ([`Lecture`], [`Registre`]), ce que lisent les vues ;
//! - les règles qui décident ce qui compte, celles de l'ERP (`accounting.py`, autour de `:3480` et `:3620`) :
//!   [`Facture::depense`], [`Facture::revenu`], le signe d'une note de crédit ([`Facture::signe`]), ce qui reste à
//!   recevoir ou à payer ([`Facture::reste`]), ce qui est en retard ([`Facture::en_retard`]) ;
//! - l'argent en centimes entiers, jamais un nombre à virgule flottante ([`Cents`], [`cents_de`], [`argent`],
//!   [`compact`]) ;
//! - la période et le chantier choisis en tête de l'onglet ([`Filtres`]), qui valent pour toutes ses vues.
//!
//! Chaque vue a son calcul ici, pur et essayé (`compta\bord.rs`, `listes.rs`, `sommaires.rs`, `historique.rs`), et son
//! dessin dans `natif\compta\`. La vitrine de l'essai du thème a son registre inventé ([`vitrine`]).

pub mod bord;
pub mod historique;
pub mod listes;
pub mod sommaires;
pub mod vitrine;

use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::centre::{date_du_numero, numero_du_jour};

/// La dernière lecture de la comptabilité, dans le dossier local.
pub const FICHIER: &str = "registre.json";
/// Les deux outils du serveur (`mcp_server.py`) : des lectures — le verbe « lister » en deuxième mot, que la garde
/// (`garde::est_lecture_constructo`) et le serveur (`mcp_profils.est_lecture`) rangent ainsi.
pub const OUTIL_DU_REGISTRE: &str = "comptabilite_lister_registre";
pub const OUTIL_DE_L_HISTORIQUE: &str = "comptabilite_lister_historique";
/// Ce qu'Eve lit au plus : au-delà, le registre se dit tronqué (les vues le disent).
pub const FACTURES_MAX: usize = 20_000;
pub const CHANGEMENTS_MAX: usize = 1_000;

/// De l'argent, en centimes : jamais un nombre à virgule flottante (100,00 $ × 9,975 % donne 9,98 $ dans EG et
/// 9,97 $ en binaire arrondi au pair : le plan du portage, `docs/PLAN_EG_VERS_ERP.md` de l'ERP, section 4).
pub type Cents = i64;

// ---- Le registre ----------------------------------------------------------------------------------------------------

/// À qui va une facture : un client (elle rapporte), un fournisseur (elle coûte). Le serveur le dit `destinataire`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cote {
    #[default]
    Client,
    Fournisseur,
}

/// Une facture du registre, telle que la rend `comptabilite_lister_registre` (les montants en centimes ici, en texte
/// au serveur). Un montant `None` : la base n'en a pas (EG les compte « sans total »).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Facture {
    pub id: i64,
    /// Le numéro de l'ERP (`FACT-2026-00104`).
    pub numero: String,
    /// Le numéro inscrit par le fournisseur sur sa facture (celui qu'EG montre pour une facture reçue).
    pub numero_fournisseur: Option<String>,
    pub cote: Cote,
    /// FACTURE, AVOIR (une note de crédit), ACOMPTE, PROFORMA… en majuscules.
    pub type_document: String,
    /// BROUILLON, ENVOYEE, PAYEE, PARTIELLEMENT_PAYEE, EN_RETARD, ANNULEE… en majuscules.
    pub statut: String,
    /// « AAAA-MM-JJ » : la date de la facture, son échéance, son paiement.
    pub date: Option<String>,
    pub echeance: Option<String>,
    pub paiement: Option<String>,
    /// Le client ou le fournisseur, nommé ; son id dans l'ERP.
    pub tiers: String,
    pub tiers_id: Option<i64>,
    /// Le chantier : le projet de l'ERP.
    pub projet_id: Option<i64>,
    pub projet: Option<String>,
    /// Le corps de métier (« 4. Toiture »…) et la fiabilité de la saisie (« sur », « a_verifier »), d'une facture
    /// reçue.
    pub section: Option<String>,
    pub fiabilite: Option<String>,
    pub sous_total: Option<Cents>,
    pub tps: Option<Cents>,
    pub tvq: Option<Cents>,
    pub total: Option<Cents>,
    pub paye: Option<Cents>,
    pub solde: Option<Cents>,
    /// Les noms des deux taxes (TPS et TVQ ; ailleurs, TVH, ou une seule).
    pub taxe1: Option<String>,
    pub taxe2: Option<String>,
    /// Des instants ISO 8601 (en UTC, suffixés `Z`, quand le serveur le sait).
    pub cree_le: Option<String>,
    pub modifie_le: Option<String>,
}

/// Une entrée du journal d'audit de la comptabilité (`accounting_audit_log`), la plus récente d'abord.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Changement {
    pub id: i64,
    /// Un instant ISO 8601.
    pub quand: String,
    /// Le courriel de la personne (vide : le système).
    pub qui: String,
    pub action: String,
    /// Ce qu'il touche (`entity_type` : une facture, une écriture, une opération bancaire…) et son id.
    pub entite: String,
    pub entite_id: Option<i64>,
    /// Le détail, en JSON compact, coupé à 300 caractères par le serveur.
    pub details: String,
}

/// Ce que la lecture a trouvé : toutes les factures (dans l'ordre des id), et l'historique.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Registre {
    pub factures: Vec<Facture>,
    /// Le nombre de factures que le serveur annonce ; `tronque` : Eve en a lu moins ([`FACTURES_MAX`]).
    pub au_serveur: u64,
    pub tronque: bool,
    /// Les colonnes que l'ERP de cette entreprise n'a pas (une base ancienne) : leurs champs sont vides.
    pub colonnes_absentes: Vec<String>,
    pub historique: Vec<Changement>,
    /// Le journal d'audit n'existe pas encore dans cette entreprise ; ou Eve n'en a lu qu'une partie.
    pub historique_absent: bool,
    pub historique_tronque: bool,
    /// Pourquoi l'historique s'est arrêté avant sa fin, dit pour la personne : une panne du serveur pendant sa lecture,
    /// qui laisse le registre lu (la QA5 du lot 13.2) ; vide quand il s'est lu jusqu'au bout, ou jusqu'à sa borne.
    pub historique_erreur: String,
}

/// `registre.json` du dossier local : l'état de la lecture et la dernière qui a réussi. Seul le chef l'écrit, d'un
/// geste, comme `entreprise.json` : la lecture qui commence (`en_cours`), qui réussit (`a_jour`), qui manque (`erreur`,
/// le registre d'avant restant), la connexion absente (`non_connecte` : rien). Les états sont ceux de
/// `entreprise.rs` (`A_JOUR`, `EN_COURS`, `ERREUR`, `NON_CONNECTE`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lecture {
    pub etat: String,
    /// « AAAA-MM-JJTHH:MM:SS », l'heure du poste, de la dernière lecture réussie.
    pub lu_le: String,
    /// Pourquoi la dernière lecture a manqué, dit pour la personne.
    pub erreur: String,
    pub registre: Option<Registre>,
}

// ---- Ce que rend le serveur -----------------------------------------------------------------------------------------

/// Un texte du serveur : `None` s'il manque ou s'il est vide.
fn texte(v: &Value, cle: &str) -> Option<String> {
    match v.get(cle) {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

fn entier(v: &Value, cle: &str) -> Option<i64> {
    match v.get(cle) {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Un montant du serveur : du texte (« 1149.75 »), jamais lu comme un nombre à virgule flottante ; un nombre JSON se lit
/// par son écriture.
fn montant(v: &Value, cle: &str) -> Option<Cents> {
    match v.get(cle) {
        Some(Value::String(s)) => cents_de(s),
        Some(Value::Number(n)) => cents_de(&n.to_string()),
        _ => None,
    }
}

/// Une date « AAAA-MM-JJ » du serveur (une heure qui la suivrait est ôtée) ; `None` si elle ne se lit pas.
fn date(v: &Value, cle: &str) -> Option<String> {
    let t = texte(v, cle)?;
    let d = t.get(..10)?;
    numero_du_jour(d).map(|_| d.to_string())
}

impl Facture {
    /// Une facture telle que la rend `comptabilite_lister_registre` (son contrat : `mcp_server.py`) ; `None` sans id.
    pub fn du_serveur(v: &Value) -> Option<Facture> {
        let id = entier(v, "id")?;
        let cote = match texte(v, "destinataire").as_deref().map(str::to_lowercase).as_deref() {
            Some("fournisseur") => Cote::Fournisseur,
            _ => Cote::Client,
        };
        Some(Facture {
            id,
            numero: texte(v, "numero").unwrap_or_default(),
            numero_fournisseur: texte(v, "numero_fournisseur"),
            cote,
            type_document: texte(v, "type_document").unwrap_or_else(|| "FACTURE".into()).to_uppercase(),
            statut: texte(v, "statut").unwrap_or_default().to_uppercase(),
            date: date(v, "date"),
            echeance: date(v, "echeance"),
            paiement: date(v, "paiement"),
            tiers: texte(v, "tiers").unwrap_or_default(),
            tiers_id: entier(v, "tiers_id"),
            projet_id: entier(v, "projet_id"),
            projet: texte(v, "projet"),
            section: texte(v, "section"),
            fiabilite: texte(v, "fiabilite"),
            sous_total: montant(v, "sous_total"),
            tps: montant(v, "tps"),
            tvq: montant(v, "tvq"),
            total: montant(v, "total"),
            paye: montant(v, "paye"),
            solde: montant(v, "solde"),
            taxe1: texte(v, "taxe1"),
            taxe2: texte(v, "taxe2"),
            cree_le: texte(v, "cree_le"),
            modifie_le: texte(v, "modifie_le"),
        })
    }

    // ---- Les règles : celles de l'ERP ----

    pub fn est_avoir(&self) -> bool {
        self.type_document.eq_ignore_ascii_case("AVOIR")
    }

    pub fn est_proforma(&self) -> bool {
        let t = self.type_document.to_uppercase().replace(['_', '-', ' '], "");
        t == "PROFORMA"
    }

    pub fn annulee(&self) -> bool {
        self.statut.eq_ignore_ascii_case("ANNULEE")
    }

    pub fn brouillon(&self) -> bool {
        self.statut.eq_ignore_ascii_case("BROUILLON")
    }

    /// Une dépense : la facture d'un fournisseur, ni annulée ni pro forma. Un brouillon compte — la facture est reçue,
    /// sa saisie attend d'être validée — et le tableau de bord dit combien il y en a.
    pub fn depense(&self) -> bool {
        self.cote == Cote::Fournisseur && !self.annulee() && !self.est_proforma()
    }

    /// Un revenu : une facture à un client, émise — ni brouillon, ni annulée, ni pro forma.
    pub fn revenu(&self) -> bool {
        self.cote == Cote::Client && !self.annulee() && !self.est_proforma() && !self.brouillon()
    }

    /// Le signe de ses montants dans une somme : une note de crédit (AVOIR), stockée positive, se retranche, comme dans
    /// le chiffre d'affaires de l'ERP (`accounting.py:3484`).
    pub fn signe(&self) -> i64 {
        if self.est_avoir() { -1 } else { 1 }
    }

    pub fn sous_total_signe(&self) -> Cents {
        self.signe() * self.sous_total.unwrap_or(0)
    }

    pub fn tps_signee(&self) -> Cents {
        self.signe() * self.tps.unwrap_or(0)
    }

    pub fn tvq_signee(&self) -> Cents {
        self.signe() * self.tvq.unwrap_or(0)
    }

    pub fn total_signe(&self) -> Cents {
        self.signe() * self.total.unwrap_or(0)
    }

    pub fn payee(&self) -> bool {
        self.statut.eq_ignore_ascii_case("PAYEE")
    }

    /// Ce qui reste à recevoir d'un client, ou à payer à un fournisseur, par la règle de l'ERP (`accounting.py:3485`
    /// et `:3628-3640`) : rien pour une note de crédit (sa réduction est déjà dans le solde de la facture d'origine,
    /// `:3476`), un brouillon, une facture annulée, pro forma ou payée ; pour une facture ouverte, son solde
    /// (`solde_du`, que le serveur rend tel qu'en base) — et, quand il vaut zéro ou manque, son total moins ce qui est
    /// payé, le repli de l'ERP pour une base où `solde_du` n'est pas tenu. Jamais négatif. (Le repli appliqué à
    /// TOUTE facture aurait fait d'une note de crédit, ou d'une facture soldée par elle, un montant « à recevoir » :
    /// l'agent de l'ERP l'a relevé, le contrat du serveur rend désormais le solde brut.)
    pub fn reste(&self) -> Cents {
        if self.est_avoir() || self.annulee() || self.est_proforma() || self.brouillon() || self.payee() {
            return 0;
        }
        let solde = match self.solde {
            Some(s) if s != 0 => s,
            _ => self.total.unwrap_or(0) - self.paye.unwrap_or(0),
        };
        solde.max(0)
    }

    /// Son échéance : celle de l'ERP, sinon un mois (trente jours) après sa date, la règle d'EG.
    pub fn echeance_effective(&self) -> Option<String> {
        if let Some(e) = &self.echeance {
            return Some(e.clone());
        }
        let d = numero_du_jour(self.date.as_deref()?)?;
        Some(date_du_numero(d + 30))
    }

    /// Les jours de retard au `aujourdhui` (« AAAA-MM-JJ ») : un reste, et son échéance passée (strictement) ; 0
    /// sinon.
    pub fn jours_de_retard(&self, aujourdhui: &str) -> i64 {
        if self.reste() <= 0 {
            return 0;
        }
        match (self.echeance_effective().as_deref().and_then(numero_du_jour), numero_du_jour(aujourdhui)) {
            (Some(e), Some(a)) if a > e => a - e,
            _ => 0,
        }
    }

    pub fn en_retard(&self, aujourdhui: &str) -> bool {
        self.jours_de_retard(aujourdhui) > 0
    }

    /// Réglée : une facture émise (encaissée d'un client, payée à un fournisseur) dont rien ne reste ([`Facture::reste`],
    /// la même règle que ce qui reste), de total positif.
    pub fn reglee(&self) -> bool {
        !self.est_avoir()
            && !self.annulee()
            && !self.est_proforma()
            && !self.brouillon()
            && self.total.unwrap_or(0) > 0
            && self.reste() == 0
    }

    /// Ce qu'une facture émise a fait entrer : ce que l'ERP dit payé (`montant_paye`, que somme son « total encaissé »,
    /// `accounting.py:3505`) ; une base qui ne le dit pas, son total moins son solde. Rien d'une facture qui n'est pas
    /// un revenu (un brouillon, une annulée, une pro forma, une facture reçue), ni d'une note de crédit : l'ERP garde
    /// son payé à zéro, « un AVOIR n'est pas une créance » (`accounting.py:11312`), et sa réduction est déjà dans le
    /// solde de la facture d'origine. Une seule règle pour le tableau de bord et la liste des clients (la fusion de
    /// l'équipe : chacun avait la sienne).
    pub fn encaisse(&self) -> Cents {
        if !self.revenu() || self.est_avoir() {
            return 0;
        }
        let paye = match (self.paye, self.total, self.solde) {
            (Some(p), _, _) => p,
            (None, Some(t), Some(s)) => t - s,
            _ => 0,
        };
        paye.max(0)
    }

    /// Le mois de sa date, « AAAA-MM ».
    pub fn mois(&self) -> Option<&str> {
        self.date.as_deref().and_then(|d| d.get(..7))
    }
}

impl Changement {
    /// Une entrée telle que la rend `comptabilite_lister_historique` ; `None` sans id.
    pub fn du_serveur(v: &Value) -> Option<Changement> {
        Some(Changement {
            id: entier(v, "id")?,
            quand: texte(v, "quand").unwrap_or_default(),
            qui: texte(v, "qui").unwrap_or_default(),
            action: texte(v, "action").unwrap_or_default(),
            entite: texte(v, "entite").unwrap_or_default(),
            entite_id: entier(v, "entite_id"),
            details: texte(v, "details").unwrap_or_default(),
        })
    }
}

// ---- L'argent ---------------------------------------------------------------------------------------------------------

/// « 1149.75 », « -12.5 », « 3 » → des centimes ; plus de deux décimales s'arrondissent au cent, la demie loin de zéro
/// (la règle d'EG) ; `None` pour ce qui n'est pas un montant, ou qui déborde.
pub fn cents_de(s: &str) -> Option<Cents> {
    let s = s.trim();
    let (negatif, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let (entiers, decimales) = s.split_once('.').unwrap_or((s, ""));
    if entiers.is_empty() && decimales.is_empty() {
        return None;
    }
    if !entiers.bytes().all(|b| b.is_ascii_digit()) || !decimales.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut c: i64 = 0;
    for b in entiers.bytes() {
        c = c.checked_mul(10)?.checked_add(i64::from(b - b'0'))?;
    }
    let mut d = decimales.bytes().map(|b| i64::from(b - b'0'));
    let dixiemes = d.next().unwrap_or(0);
    let centiemes = d.next().unwrap_or(0);
    let arrondi = i64::from(d.next().is_some_and(|x| x >= 5));
    c = c.checked_mul(100)?.checked_add(dixiemes * 10 + centiemes + arrondi)?;
    Some(if negatif { -c } else { c })
}

/// Les chiffres d'un entier, une espace fine entre les milliers (comme `centre::milliers`).
pub(crate) fn par_milliers(n: u64) -> String {
    let t = n.to_string();
    let mut s = String::new();
    for (i, c) in t.chars().enumerate() {
        if i > 0 && (t.len() - i).is_multiple_of(3) {
            s.push('\u{202f}');
        }
        s.push(c);
    }
    s
}

/// 123450 → « 1 234,50 $ » (une espace fine entre les milliers, une insécable avant le dollar) ; un négatif prend le
/// signe moins typographique, comme EG (« −45,00 $ »).
pub fn argent(c: Cents) -> String {
    let n = c.unsigned_abs();
    let signe = if c < 0 { "\u{2212}" } else { "" };
    format!("{signe}{},{:02}\u{a0}$", par_milliers(n / 100), n % 100)
}

/// « 12,3 k$ », « 1,2 M$ », « 850 $ » : pour les axes et le centre d'un anneau, au dixième, la demie arrondie vers le
/// haut — d'après les CENTIMES, et l'unité choisie APRÈS l'arrondi : 999 950 $ se dit « 1 M$ » (pas « 1000 k$ »),
/// 999,50 $ « 1 k$ », −0,01 $ « 0 $ » (le `compactC` d'EG tronquait d'abord les cents ; la QA5 du lot 13.2).
pub fn compact(c: Cents) -> String {
    let n = c.unsigned_abs();
    let dixiemes = |u: u64| (n + u / 2) / u;
    let en = |x: u64, s: &str| {
        let decimale = if x % 10 != 0 { format!(",{}", x % 10) } else { String::new() };
        format!("{}{decimale}\u{a0}{s}", par_milliers(x / 10))
    };
    // Des centimes : 100 font un dollar, 10 000 un dixième de millier, 10 000 000 un dixième de million.
    let dollars = (n + 50) / 100;
    let corps = if dollars < 1_000 {
        format!("{dollars}\u{a0}$")
    } else if dixiemes(10_000) < 10_000 {
        en(dixiemes(10_000), "k$")
    } else {
        en(dixiemes(10_000_000), "M$")
    };
    if c < 0 && corps != "0\u{a0}$" { format!("\u{2212}{corps}") } else { corps }
}

/// Une somme de factures : combien, et leurs montants signés (une note de crédit se retranche) ; `sans_total` : celles
/// dont la base n'a pas de total.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Somme {
    pub n: u32,
    pub sous_total: Cents,
    pub tps: Cents,
    pub tvq: Cents,
    pub total: Cents,
    pub sans_total: u32,
}

impl Somme {
    pub fn ajouter(&mut self, f: &Facture) {
        self.n += 1;
        self.sous_total += f.sous_total_signe();
        self.tps += f.tps_signee();
        self.tvq += f.tvq_signee();
        self.total += f.total_signe();
        if f.total.is_none() {
            self.sans_total += 1;
        }
    }

    pub fn de<'a>(factures: impl IntoIterator<Item = &'a Facture>) -> Somme {
        let mut s = Somme::default();
        for f in factures {
            s.ajouter(f);
        }
        s
    }
}

// ---- La période et le chantier ----------------------------------------------------------------------------------------

/// Les mois en français, pour les libellés.
pub const MOIS: [&str; 12] = [
    "janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre",
    "décembre",
];
pub const MOIS_COURTS: [&str; 12] =
    ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];

/// La période choisie en tête de l'onglet, comme le tableau de bord d'EG : toute la période, les 12 derniers mois (le
/// mois en cours et les onze d'avant), une année, un mois.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Periode {
    #[default]
    Tout,
    DouzeMois,
    Annee(i32),
    Mois(i32, u8),
}

/// « AAAA-MM » d'une date, ses nombres.
fn annee_mois(date: &str) -> Option<(i32, u8)> {
    let a = date.get(..4)?.parse().ok()?;
    let m: u8 = date.get(5..7)?.parse().ok()?;
    (1..=12).contains(&m).then_some((a, m))
}

impl Periode {
    /// Une date dans la période, `aujourdhui` (« AAAA-MM-JJ ») pour repère. Une facture sans date n'est que dans
    /// « Tout » (comme dans EG).
    pub fn contient(&self, date: Option<&str>, aujourdhui: &str) -> bool {
        if *self == Periode::Tout {
            return true;
        }
        let Some((a, m)) = date.and_then(annee_mois) else { return false };
        match *self {
            Periode::Tout => true,
            Periode::Annee(x) => a == x,
            Periode::Mois(x, y) => a == x && m == y,
            Periode::DouzeMois => {
                let Some((ya, ym)) = annee_mois(aujourdhui) else { return false };
                // Le premier des douze mois : onze mois avant celui d'aujourd'hui.
                // Et le dernier, celui d'aujourd'hui : une facture datée d'un mois à venir n'est pas des douze derniers
                // (la QA5 du lot 13.2 : quatorze mois dessinés sous « Les 12 derniers mois »).
                let courant = ya * 12 + i32::from(ym) - 1;
                (courant - 11..=courant).contains(&(a * 12 + i32::from(m) - 1))
            }
        }
    }

    /// Ce que dit le choix : « Toute la période », « 12 derniers mois », « 2026 », « novembre 2026 ».
    pub fn libelle(&self) -> String {
        match *self {
            Periode::Tout => "Toute la période".into(),
            Periode::DouzeMois => "12 derniers mois".into(),
            Periode::Annee(a) => a.to_string(),
            Periode::Mois(a, m) => format!("{} {a}", MOIS[usize::from(m.clamp(1, 12)) - 1]),
        }
    }
}

/// Le chantier choisi : tous, un projet de l'ERP, ou les factures sans chantier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Chantier {
    #[default]
    Tous,
    Projet(i64),
    Aucun,
}

impl Chantier {
    pub fn contient(&self, f: &Facture) -> bool {
        match *self {
            Chantier::Tous => true,
            Chantier::Projet(id) => f.projet_id == Some(id),
            Chantier::Aucun => f.projet_id.is_none(),
        }
    }
}

/// Ce que l'onglet a choisi en tête, pour toutes ses vues.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Filtres {
    pub periode: Periode,
    pub chantier: Chantier,
}

impl Filtres {
    /// La facture est-elle dans la période et le chantier choisis ?
    pub fn retient(&self, f: &Facture, aujourdhui: &str) -> bool {
        self.chantier.contient(f) && self.periode.contient(f.date.as_deref(), aujourdhui)
    }
}

/// Les chantiers du registre, pour le choix : (id, nom), dans l'ordre des noms ; et s'il y a des factures sans chantier.
pub fn chantiers(r: &Registre) -> (Vec<(i64, String)>, bool) {
    let mut vus: Vec<(i64, String)> = Vec::new();
    let mut sans = false;
    for f in &r.factures {
        match f.projet_id {
            Some(id) if !vus.iter().any(|(i, _)| *i == id) => {
                vus.push((id, f.projet.clone().unwrap_or_else(|| format!("Projet {id}"))));
            }
            Some(_) => {}
            None => sans = true,
        }
    }
    // Sans accents ni casse : « École » entre « Entrepôt » et « Projet 11 », pas après (la QA5 du lot 13.2).
    vus.sort_by_cached_key(|(id, nom)| (eve::parole::normaliser(nom), *id));
    (vus, sans)
}

/// Le nom d'une taxe, comparable : en capitales, sans points ni espaces, l'anglais rendu au français (« t.p.s. » et
/// « GST » → « TPS », « HST » → « TVH », « QST » → « TVQ »).
fn taxe_comparable(t: &str) -> String {
    let n: String = t.to_uppercase().chars().filter(|c| !matches!(c, '.' | ' ' | '-' | '\u{a0}')).collect();
    match n.as_str() {
        "GST" => "TPS".into(),
        "HST" => "TVH".into(),
        "QST" => "TVQ".into(),
        _ => n,
    }
}

/// Les taxes qui se récupèrent (les crédits et les remboursements de taxe sur les intrants) : la TPS, la TVH, la TVQ.
/// Leur nette se conclut « à remettre » ou « en faveur de l'entreprise » ; la TVP de l'Ouest ou la taxe de vente d'un
/// État américain, payées à un fournisseur, ne reviennent pas.
const RECUPERABLES: [&str; 3] = ["TPS", "TVH", "TVQ"];

/// Au plus, les taxes nommées d'une même case (« TPS/TVH ») ; au-delà, « … ».
const TAXES_PAR_CASE: usize = 3;

/// Les noms des taxes d'un ensemble de factures, comptés une facture à la fois ([`NomsDesTaxes::compter`]) : la règle
/// des en-têtes des listes et des sommaires (sur tout le registre, [`noms_des_taxes`]) et du tableau de bord (sur ses
/// factures retenues).
#[derive(Clone, Debug, Default)]
pub struct NomsDesTaxes {
    /// Les deux cases de taxe d'une facture (`taxe1`, `taxe2`) : chaque taxe, par son nom comparable, et ses graphies
    /// comptées.
    cases: [std::collections::HashMap<String, std::collections::HashMap<String, u32>>; 2],
    /// Une taxe nommée qui ne se récupère pas.
    d_ailleurs: bool,
}

impl NomsDesTaxes {
    pub fn compter(&mut self, f: &Facture) {
        for (case, nom) in self.cases.iter_mut().zip([&f.taxe1, &f.taxe2]) {
            if let Some(t) = nom.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                let taxe = taxe_comparable(t);
                self.d_ailleurs |= !RECUPERABLES.contains(&taxe.as_str());
                *case.entry(taxe).or_default().entry(t.to_string()).or_default() += 1;
            }
        }
    }

    /// Les noms des deux cases : chacune, ses taxes de la plus fréquente à la moins fréquente (à égalité, dans l'ordre),
    /// chacune sous sa graphie la plus fréquente (à égalité, la première dans l'ordre). Deux taxes dans la même case (la
    /// TPS au Québec, la TVH d'un chantier en Ontario) se disent ensemble, « TPS/TVH » : leur somme ne se range jamais
    /// sous le nom d'une seule (la QA5, passe 2). La TPS et la TVQ d'office (une base qui ne les nomme pas) ; une seule
    /// case nommée (la TVH de l'Ontario, la « Sales Tax » des États-Unis), la seconde VIDE — jamais une « TVQ »
    /// inventée (la QA5, passe 1).
    pub fn noms(&self) -> (String, String) {
        type Case = std::collections::HashMap<String, std::collections::HashMap<String, u32>>;
        let dire = |case: &Case| -> Option<String> {
            let mut taxes: Vec<(u32, &str)> = case
                .values()
                .map(|graphies| {
                    let n = graphies.values().sum();
                    let graphie = graphies.iter().max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)));
                    (n, graphie.map_or("", |g| g.0.as_str()))
                })
                .collect();
            taxes.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
            let mut noms: Vec<&str> = taxes.iter().take(TAXES_PAR_CASE).map(|t| t.1).collect();
            if taxes.len() > TAXES_PAR_CASE {
                noms.push("…");
            }
            (!noms.is_empty()).then(|| noms.join("/"))
        };
        match (dire(&self.cases[0]), dire(&self.cases[1])) {
            (None, None) => ("TPS".into(), "TVQ".into()),
            (a, b) => (a.unwrap_or_else(|| "TPS".into()), b.unwrap_or_default()),
        }
    }

    /// Les taxes nommées se récupèrent-elles toutes ([`RECUPERABLES`]) ? Sans aucun nom (une base qui ne les garde pas,
    /// une facture du MOBILE), la TPS et la TVQ d'office : oui (la QA5, passe 2).
    pub fn recuperables(&self) -> bool {
        !self.d_ailleurs
    }
}

/// Les noms des deux taxes du registre ([`NomsDesTaxes::noms`]) : les en-têtes des listes et des sommaires.
pub fn noms_des_taxes(r: &Registre) -> (String, String) {
    let mut noms = NomsDesTaxes::default();
    for f in &r.factures {
        noms.compter(f);
    }
    noms.noms()
}

/// Les années qui portent une facture, de la plus récente à la plus ancienne (le choix de la période).
pub fn annees(r: &Registre) -> Vec<i32> {
    let mut a: Vec<i32> =
        r.factures.iter().filter_map(|f| f.date.as_deref().and_then(annee_mois)).map(|x| x.0).collect();
    a.sort_unstable_by(|x, y| y.cmp(x));
    a.dedup();
    a
}

// ---- La vitrine de l'essai du thème ---------------------------------------------------------------------------------

/// Une lecture imposée : la vitrine de `eve --essai-theme` (une entreprise inventée). Tant qu'elle l'est, l'onglet la
/// montre à la place de celle du centre. Eve ne l'impose jamais.
static IMPOSEE: Mutex<Option<Arc<Lecture>>> = Mutex::new(None);

/// Impose cette lecture à l'onglet (la vitrine de l'essai du thème), ou rend la main au centre (`None`).
pub fn imposer(l: Option<Lecture>) {
    *IMPOSEE.lock().unwrap_or_else(PoisonError::into_inner) = l.map(Arc::new);
}

/// La lecture imposée, s'il y en a une.
pub fn imposee() -> Option<Arc<Lecture>> {
    IMPOSEE.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

#[cfg(test)]
mod essais {
    use serde_json::json;

    use super::*;

    #[test]
    fn les_montants_en_centimes_sans_virgule_flottante() {
        assert_eq!(cents_de("1149.75"), Some(114_975));
        assert_eq!(cents_de("0.10"), Some(10));
        assert_eq!(cents_de("0.1"), Some(10));
        assert_eq!(cents_de("3"), Some(300));
        assert_eq!(cents_de("-12.5"), Some(-1250));
        assert_eq!(cents_de("+7.05"), Some(705));
        assert_eq!(cents_de(".5"), Some(50));
        // Plus de deux décimales : la demie loin de zéro (9,975 → 9,98, comme EG ; −9,975 → −9,98).
        assert_eq!(cents_de("9.975"), Some(998));
        assert_eq!(cents_de("-9.975"), Some(-998));
        assert_eq!(cents_de("9.974"), Some(997));
        // Ce qui n'est pas un montant.
        for s in ["", "-", ".", "1,5", "abc", "1.2.3", "1e3", " 12 3 ", "99999999999999999999"] {
            assert_eq!(cents_de(s), None, "{s:?}");
        }
    }

    #[test]
    fn l_argent_a_la_francaise() {
        assert_eq!(argent(114_975), "1\u{202f}149,75\u{a0}$");
        assert_eq!(argent(5), "0,05\u{a0}$");
        assert_eq!(argent(0), "0,00\u{a0}$");
        assert_eq!(argent(-4500), "\u{2212}45,00\u{a0}$");
        assert_eq!(argent(123_456_789), "1\u{202f}234\u{202f}567,89\u{a0}$");
        assert_eq!(compact(85_000), "850\u{a0}$");
        assert_eq!(compact(1_234_500), "12,3\u{a0}k$");
        assert_eq!(compact(1_200_000), "12\u{a0}k$");
        assert_eq!(compact(125_000_000), "1,3\u{a0}M$");
        assert_eq!(compact(-1_234_500), "\u{2212}12,3\u{a0}k$");
        // L'unité choisie après l'arrondi, d'après les centimes (la QA5 du lot 13.2).
        assert_eq!(compact(99_995_000), "1\u{a0}M$", "999 950 $");
        assert_eq!(compact(99_950), "1\u{a0}k$", "999,50 $");
        assert_eq!(compact(99_949), "999\u{a0}$", "999,49 $");
        assert_eq!(compact(-1), "0\u{a0}$", "jamais « −0 $ »");
        assert_eq!(compact(i64::MAX), "92\u{202f}233\u{202f}720\u{202f}368,5\u{a0}M$");
    }

    fn facture(cote: Cote, statut: &str, type_document: &str, total: i64, solde: i64) -> Facture {
        Facture {
            id: 1,
            cote,
            statut: statut.into(),
            type_document: type_document.into(),
            date: Some("2026-09-15".into()),
            sous_total: Some(total * 100 / 115),
            total: Some(total),
            solde: Some(solde),
            ..Facture::default()
        }
    }

    #[test]
    fn ce_qui_compte_les_regles_de_l_erp() {
        use Cote::*;
        // Une dépense : un fournisseur, brouillon compris, ni annulée ni pro forma.
        assert!(facture(Fournisseur, "BROUILLON", "FACTURE", 100, 100).depense());
        assert!(!facture(Fournisseur, "ANNULEE", "FACTURE", 100, 100).depense());
        assert!(!facture(Fournisseur, "ENVOYEE", "PROFORMA", 100, 100).depense());
        assert!(!facture(Client, "ENVOYEE", "FACTURE", 100, 100).depense());
        // Un revenu : un client, émise.
        assert!(facture(Client, "ENVOYEE", "FACTURE", 100, 100).revenu());
        assert!(!facture(Client, "BROUILLON", "FACTURE", 100, 100).revenu());
        assert!(!facture(Client, "ENVOYEE", "PRO_FORMA", 100, 100).revenu());
        // Une note de crédit se retranche ; son solde ne reste jamais à recevoir.
        let avoir = facture(Client, "ENVOYEE", "AVOIR", 5000, 5000);
        assert_eq!((avoir.signe(), avoir.total_signe(), avoir.reste()), (-1, -5000, 0));
        // Le reste et le retard.
        let mut f = facture(Client, "ENVOYEE", "FACTURE", 10_000, 4_000);
        f.echeance = Some("2026-10-15".into());
        assert_eq!(f.reste(), 4_000);
        assert_eq!(f.jours_de_retard("2026-10-15"), 0, "le jour de l'échéance n'est pas un retard");
        assert_eq!(f.jours_de_retard("2026-10-18"), 3);
        f.echeance = None;
        assert_eq!(f.echeance_effective().as_deref(), Some("2026-10-15"), "trente jours après le 15 septembre");
        assert!(!facture(Client, "BROUILLON", "FACTURE", 100, 100).en_retard("2027-01-01"));
        assert!(facture(Client, "PAYEE", "FACTURE", 100, 0).reglee());
        assert!(!facture(Client, "ENVOYEE", "FACTURE", 100, 1).reglee());
    }

    #[test]
    fn le_reste_et_l_encaisse_suivent_l_erp() {
        use Cote::*;
        // Une facture ouverte dont le solde n'est pas tenu (zéro) : son total moins ce qui est payé, le repli de l'ERP.
        let mut f = facture(Client, "ENVOYEE", "FACTURE", 10_000, 0);
        f.paye = Some(4_000);
        assert_eq!((f.reste(), f.reglee()), (6_000, false));
        // Payée : rien ne reste, quoi que dise le solde.
        let mut p = facture(Client, "PAYEE", "FACTURE", 10_000, 2_500);
        p.paye = Some(7_500);
        assert_eq!((p.reste(), p.reglee()), (0, true));
        // Une note de crédit (son solde et son payé à zéro dans l'ERP) : rien à recevoir, rien d'encaissé ; une facture
        // soldée par elle, payée : rien non plus.
        let mut a = facture(Client, "ENVOYEE", "AVOIR", 5_000, 0);
        a.paye = Some(0);
        assert_eq!((a.reste(), a.encaisse(), a.reglee()), (0, 0, false));
        // L'encaissé : le payé d'un revenu ; sans payé, son total moins son solde ; rien d'un brouillon, rien d'une
        // facture reçue.
        let mut e = facture(Client, "PARTIELLEMENT_PAYEE", "FACTURE", 10_000, 6_000);
        e.paye = Some(4_000);
        assert_eq!(e.encaisse(), 4_000);
        e.paye = None;
        assert_eq!(e.encaisse(), 4_000);
        assert_eq!(facture(Client, "BROUILLON", "FACTURE", 100, 100).encaisse(), 0);
        let mut r = facture(Fournisseur, "PAYEE", "FACTURE", 100, 0);
        r.paye = Some(100);
        assert_eq!((r.encaisse(), r.reste(), r.reglee()), (0, 0, true));
    }

    #[test]
    fn les_noms_des_taxes() {
        let f = |t1: Option<&str>, t2: Option<&str>| Facture {
            id: 1,
            taxe1: t1.map(Into::into),
            taxe2: t2.map(Into::into),
            ..Facture::default()
        };
        let r = |v: Vec<Facture>| Registre { factures: v, ..Registre::default() };
        let n = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(noms_des_taxes(&r(vec![])), n("TPS", "TVQ"), "une base qui ne les nomme pas");
        assert_eq!(noms_des_taxes(&r(vec![f(Some("TVH"), None), f(Some(" TVH "), Some("  "))])), n("TVH", ""));
        // La GST est la TPS (sous sa graphie la plus fréquente) ; la PST, une autre taxe que la TVQ : elle se nomme.
        let melange = vec![f(Some("TPS"), Some("TVQ")), f(Some("GST"), Some("PST")), f(Some("TPS"), Some("TVQ"))];
        assert_eq!(noms_des_taxes(&r(melange)), n("TPS", "TVQ/PST"), "les plus fréquents d'abord");
        // La QA5 (passe 2) : un chantier en Ontario, sa TVH sous le nom « TPS ».
        let ontario = vec![f(Some("TPS"), Some("TVQ")), f(Some("TVH"), None), f(Some("t.p.s."), Some("TVQ"))];
        assert_eq!(noms_des_taxes(&r(ontario)), n("TPS/TVH", "TVQ"));
        let toutes = ["TPS", "TVH", "TVP", "Sales Tax", "TVH"].map(|t| f(Some(t), None)).to_vec();
        assert_eq!(noms_des_taxes(&r(toutes)), n("TVH/Sales Tax/TPS/…", ""), "au plus trois, puis « … »");
        let mut compte = NomsDesTaxes::default();
        for (t1, t2) in [(Some("GST"), Some("QST")), (Some("HST"), None), (None, None)] {
            compte.compter(&f(t1, t2));
        }
        assert!(compte.recuperables(), "la TPS, la TVH et la TVQ se récupèrent");
        compte.compter(&f(Some("TPS"), Some("TVP")));
        assert!(!compte.recuperables(), "la TVP ne se récupère pas");
        assert!(NomsDesTaxes::default().recuperables(), "sans aucun nom, la TPS et la TVQ d'office");
    }

    #[test]
    fn les_periodes() {
        let a = "2026-11-02";
        assert!(Periode::Tout.contient(None, a));
        assert!(!Periode::DouzeMois.contient(None, a));
        assert!(Periode::DouzeMois.contient(Some("2025-12-01"), a), "le premier des douze mois");
        assert!(!Periode::DouzeMois.contient(Some("2025-11-30"), a), "le treizième");
        assert!(Periode::DouzeMois.contient(Some("2026-11-30"), a), "le mois en cours");
        assert!(!Periode::DouzeMois.contient(Some("2026-12-01"), a), "un mois à venir");
        assert!(Periode::Annee(2026).contient(Some("2026-01-01"), a));
        assert!(!Periode::Annee(2026).contient(Some("2025-12-31"), a));
        assert!(Periode::Mois(2026, 10).contient(Some("2026-10-31"), a));
        assert!(!Periode::Mois(2026, 10).contient(Some("2026-11-01"), a));
        assert_eq!(Periode::Mois(2026, 8).libelle(), "août 2026");
    }

    #[test]
    fn une_facture_du_serveur() {
        let v = json!({"id": 104, "numero": "FACT-2026-00104", "numero_fournisseur": null, "destinataire": "fournisseur",
            "type_document": "facture", "statut": "envoyee", "date": "2026-11-01", "echeance": "2026-12-01T00:00:00",
            "paiement": "", "tiers": "Béton Fictif ltée", "tiers_id": 12, "projet_id": 7, "projet": "Entrepôt",
            "section": "2. Fondation", "fiabilite": "sur", "sous_total": "1000.00", "tps": "50.00", "tvq": "99.75",
            "total": "1149.75", "paye": "0.00", "solde": "1149.75", "taxe1": "TPS", "taxe2": "TVQ",
            "cree_le": "2026-11-01T15:12:00Z", "modifie_le": null});
        let f = Facture::du_serveur(&v).unwrap();
        assert_eq!(
            (f.id, f.cote, f.type_document.as_str(), f.statut.as_str()),
            (104, Cote::Fournisseur, "FACTURE", "ENVOYEE")
        );
        assert_eq!((f.total, f.tvq, f.paye), (Some(114_975), Some(9_975), Some(0)));
        assert_eq!((f.echeance.as_deref(), f.paiement.as_deref()), (Some("2026-12-01"), None));
        assert_eq!(f.numero_fournisseur, None);
        assert!(Facture::du_serveur(&json!({"numero": "sans id"})).is_none());
        // Un montant absent ou illisible reste absent : jamais 0 en silence.
        let g = Facture::du_serveur(&json!({"id": 1, "total": null, "tps": "x"})).unwrap();
        assert_eq!((g.total, g.tps, g.cote), (None, None, Cote::Client));
    }
}

/// La QA5 (mutation, passe 1) : les essais qui manquaient. Chacun tient le comportement d'aujourd'hui, et a rougi sous
/// le mutant de son nom.
#[cfg(test)]
mod manques_qa {
    use serde_json::json;

    use super::*;

    const AUJ: &str = "2026-11-02";

    fn fac(id: i64, cote: Cote, statut: &str, date: &str, total: Option<Cents>) -> Facture {
        Facture {
            id,
            numero: format!("N-{id}"),
            cote,
            type_document: "FACTURE".into(),
            statut: statut.into(),
            date: Some(date.into()),
            tiers: format!("Tiers {id}"),
            sous_total: total,
            tps: total.map(|_| 0),
            tvq: total.map(|_| 0),
            total,
            paye: total.map(|_| 0),
            solde: total,
            taxe1: Some("TPS".into()),
            taxe2: Some("TVQ".into()),
            ..Facture::default()
        }
    }

    fn reg(factures: Vec<Facture>) -> Registre {
        Registre { factures, ..Registre::default() }
    }

    #[test]
    fn manque_1_un_texte_blanc_du_serveur_reste_absent() {
        let f = Facture::du_serveur(&json!({"id": 1, "projet_id": 7, "projet": "  ", "section": "", "fiabilite": " "}))
            .unwrap();
        assert_eq!((f.projet.clone(), f.section.clone(), f.fiabilite.clone()), (None, None, None));
        assert_eq!(chantiers(&reg(vec![f])).0, vec![(7, "Projet 7".to_string())]);
    }

    #[test]
    fn manque_2_les_textes_du_serveur_sont_rognes() {
        let f = Facture::du_serveur(&json!({"id": 1, "tiers": " Beton Fictif ", "statut": "payee "})).unwrap();
        assert_eq!((f.tiers.as_str(), f.statut.as_str(), f.payee()), ("Beton Fictif", "PAYEE", true));
    }

    #[test]
    fn manque_4_5_7_8_ce_que_le_serveur_ecrirait_autrement() {
        let f = Facture::du_serveur(
            &json!({"id": "104", "total": 1149.75, "date": "2026-13-45", "destinataire": "Fournisseur"}),
        )
        .unwrap();
        assert_eq!((f.id, f.total, f.date.clone(), f.cote), (104, Some(114_975), None, Cote::Fournisseur));
    }

    #[test]
    fn manque_38_39_un_solde_negatif_ne_laisse_rien_a_recevoir() {
        let mut f = fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(10_000));
        (f.paye, f.solde) = (None, Some(-5_000));
        assert_eq!(f.reste(), 0);
        let s = sommaires::par_fournisseur(
            &reg(vec![f, fac(2, Cote::Fournisseur, "ENVOYEE", "2026-10-02", Some(3_000))]),
            &Filtres::default(),
            AUJ,
        );
        assert_eq!(s.total.reste, 3_000);
    }

    #[test]
    fn manque_46_227_229_une_facture_sans_total_n_est_ni_payee_ni_encaissee() {
        let f = fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", None);
        assert!(!f.reglee());
        assert_eq!(listes::reste_du_fournisseur(&f, AUJ), listes::Reste::Rien);
        let c = fac(2, Cote::Client, "ENVOYEE", "2026-10-01", None);
        assert_eq!(listes::paiement_du_client(&c, AUJ), listes::Paiement::Rien);
    }

    #[test]
    fn manque_51_l_encaisse_est_le_paye_meme_quand_le_solde_n_est_pas_tenu() {
        let mut f = fac(1, Cote::Client, "PARTIELLEMENT_PAYEE", "2026-10-01", Some(10_000));
        (f.paye, f.solde) = (Some(4_000), Some(0));
        assert_eq!(f.encaisse(), 4_000);
    }

    #[test]
    fn manque_53_une_annulee_un_brouillon_une_pro_forma_n_encaissent_rien() {
        for (statut, sorte) in [("ANNULEE", "FACTURE"), ("BROUILLON", "FACTURE"), ("ENVOYEE", "PROFORMA")] {
            let mut f = fac(1, Cote::Client, statut, "2026-10-01", Some(10_000));
            f.type_document = sorte.into();
            (f.paye, f.solde) = (Some(10_000), Some(0));
            assert_eq!(f.encaisse(), 0, "{statut} {sorte}");
        }
    }

    #[test]
    fn manque_63_l_arrondi_ne_lit_que_la_troisieme_decimale() {
        assert_eq!(cents_de("9.9705"), Some(997));
        assert_eq!(cents_de("1149.7449"), Some(114_974));
    }

    #[test]
    fn manque_65_un_montant_qui_deborde_au_centime_n_en_est_pas_un() {
        assert_eq!(cents_de("99999999999999999"), None);
    }

    #[test]
    fn manque_76_77_compact_aux_bornes_exactes() {
        assert_eq!(compact(100_000_000), "1\u{a0}M$");
        assert_eq!(compact(100_000), "1\u{a0}k$");
    }

    #[test]
    fn manque_80_sans_total_ne_compte_que_le_total_absent() {
        let mut b = fac(2, Cote::Client, "ENVOYEE", "2026-10-01", Some(100));
        b.sous_total = None;
        assert_eq!(Somme::de([&b]).sans_total, 0);
    }

    #[test]
    fn manque_89_un_mois_est_celui_de_son_annee() {
        assert!(!Periode::Mois(2026, 10).contient(Some("2025-10-15"), AUJ));
    }

    #[test]
    fn manque_94_95_96_97_les_choix_d_en_tete() {
        let mut a = fac(1, Cote::Client, "ENVOYEE", "2025-03-01", Some(1));
        (a.projet_id, a.projet) = (Some(9), Some("atelier".into()));
        let mut b = fac(2, Cote::Client, "ENVOYEE", "2026-03-01", Some(1));
        (b.projet_id, b.projet) = (Some(7), Some("Bureau".into()));
        let c = fac(3, Cote::Client, "ENVOYEE", "2026-05-01", Some(1));
        let r = reg(vec![a, b, c]);
        assert_eq!(chantiers(&r), (vec![(9, "atelier".to_string()), (7, "Bureau".to_string())], true));
        assert_eq!(annees(&r), vec![2026, 2025]);
    }

    #[test]
    fn manque_102_un_brouillon_client_seul_n_inscrit_pas_de_clients() {
        let r = reg(vec![
            fac(1, Cote::Client, "BROUILLON", "2026-10-01", Some(1_000)),
            fac(2, Cote::Fournisseur, "ENVOYEE", "2026-10-02", Some(500)),
        ]);
        let b = bord::calculer(&r, Filtres::default(), AUJ);
        assert!(!b.clients_inscrits);
        assert!(bord::indicateurs_des_revenus(&b, Periode::Tout).is_empty());
    }

    #[test]
    fn manque_106_108_gst_qst_et_minuscules_sont_la_tps_et_la_tvq() {
        for (t1, t2) in [("GST", "QST"), ("tps", "tvq")] {
            let mut f = fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(1_000));
            (f.taxe1, f.taxe2) = (Some(t1.into()), Some(t2.into()));
            assert!(bord::calculer(&reg(vec![f]), Filtres::default(), AUJ).taxes_recuperables, "{t1} {t2}");
        }
    }

    #[test]
    fn manque_114_chaque_facture_retenue_a_son_mois_dessine() {
        let r = reg(vec![
            fac(1, Cote::Fournisseur, "ENVOYEE", "2027-03-15", Some(10_000)),
            fac(2, Cote::Fournisseur, "ENVOYEE", "2026-10-15", Some(10_000)),
        ]);
        let b = bord::calculer(&r, Filtres { periode: Periode::DouzeMois, ..Filtres::default() }, AUJ);
        assert_eq!(b.mois.iter().map(|m| m.depenses).sum::<Cents>(), b.depenses.total);
    }

    #[test]
    fn manque_117_un_mois_qui_n_a_qu_une_facture_sans_total_compte_dans_la_moyenne() {
        let r = reg(vec![
            fac(1, Cote::Fournisseur, "ENVOYEE", "2026-09-10", None),
            fac(2, Cote::Fournisseur, "ENVOYEE", "2026-10-10", Some(10_000)),
        ]);
        let b = bord::calculer(&r, Filtres::default(), AUJ);
        assert_eq!((b.mois_actifs, b.moyenne), (2, 5_000));
    }

    #[test]
    fn manque_123_le_premier_mois_dessine_n_est_pas_cache() {
        let r = reg(vec![
            fac(1, Cote::Fournisseur, "ENVOYEE", "2022-01-15", Some(100)),
            fac(2, Cote::Client, "ENVOYEE", "2022-02-15", Some(7_000)),
            fac(3, Cote::Fournisseur, "ENVOYEE", "2022-03-15", Some(500)),
            fac(4, Cote::Client, "ENVOYEE", "2022-03-20", Some(9_000)),
            fac(5, Cote::Fournisseur, "ENVOYEE", "2026-02-15", Some(1)),
        ]);
        let b = bord::calculer(&r, Filtres::default(), AUJ);
        assert_eq!((b.mois[0].cle(), b.mois_caches), ("2022-03".to_string(), 2));
        assert_eq!((b.mois_caches_avec_depenses, b.revenus_caches), (1, (1, 7_000)));
    }

    #[test]
    fn manque_124_douze_mois_qui_commencent_avec_un_trimestre_le_gardent_entier() {
        let r = reg(vec![fac(1, Cote::Fournisseur, "ENVOYEE", "2026-02-10", Some(10_000))]);
        let b = bord::calculer(&r, Filtres { periode: Periode::DouzeMois, ..Filtres::default() }, "2026-12-15");
        assert_eq!(b.trimestre_coupe, None);
        let t: Vec<String> = b.trimestres.iter().map(bord::Trimestre::court).collect();
        assert_eq!(t, ["T1 2026", "T2 2026", "T3 2026", "T4 2026"]);
    }

    #[test]
    fn manque_126_un_seul_trimestre_se_montre() {
        let r = reg(vec![fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-05", Some(10_000))]);
        let b = bord::calculer(&r, Filtres::default(), AUJ);
        assert_eq!(b.trimestres.iter().map(bord::Trimestre::court).collect::<Vec<_>>(), ["T4 2026"]);
    }

    #[test]
    fn manque_129_les_chantiers_sans_depense_se_rangent_par_leurs_revenus() {
        let mut a = fac(1, Cote::Client, "ENVOYEE", "2026-10-01", Some(10_000));
        (a.projet_id, a.projet) = (Some(1), Some("Alpha".into()));
        let mut b = fac(2, Cote::Client, "ENVOYEE", "2026-10-02", Some(50_000));
        (b.projet_id, b.projet) = (Some(2), Some("Beta".into()));
        let bo = bord::calculer(&reg(vec![a, b]), Filtres::default(), AUJ);
        assert_eq!(bo.chantiers.iter().map(|l| l.nom.as_str()).collect::<Vec<_>>(), ["Beta", "Alpha"]);
    }

    #[test]
    fn manque_130_131_douze_chantiers_et_dix_fournisseurs_tout_juste() {
        let factures = (1..=12)
            .map(|k| {
                let mut f = fac(k, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(k * 100));
                (f.projet_id, f.projet, f.tiers) = (Some(k), Some(format!("C{k:02}")), format!("F{}", k % 10));
                f
            })
            .collect();
        let b = bord::calculer(&reg(factures), Filtres::default(), AUJ);
        assert_eq!((b.chantiers.len(), b.autres_chantiers.is_none()), (12, true));
        assert_eq!((b.fournisseurs.len(), b.autres_fournisseurs.is_none()), (10, true));
    }

    #[test]
    fn manque_134_a_egalite_le_premier_nom_de_taxe() {
        let mut a = fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(100));
        a.taxe1 = Some("GST".into());
        let b = fac(2, Cote::Fournisseur, "ENVOYEE", "2026-10-02", Some(100));
        assert_eq!(bord::calculer(&reg(vec![a, b]), Filtres::default(), AUJ).taxes.0, "GST");
    }

    #[test]
    fn manque_136_140_141_146_148_ce_que_dit_le_tableau_de_bord() {
        assert_eq!(bord::part(100, 1_000).as_deref(), Some("10\u{a0}%"));
        let mut payee = fac(1, Cote::Client, "PAYEE", "2026-10-01", Some(10_000));
        (payee.paye, payee.solde) = (Some(10_000), Some(0));
        let depense = fac(2, Cote::Fournisseur, "ENVOYEE", "2026-09-01", Some(10_000));
        let b = bord::calculer(&reg(vec![payee, depense]), Filtres::default(), AUJ);
        let i = bord::indicateurs_des_revenus(&b, Periode::Tout);
        assert!(i[1].detail.starts_with("100\u{a0}% du factur"), "{}", i[1].detail);
        assert!(!i[3].alerte, "une marge nulle : {}", i[3].valeur);
        assert!(bord::dernier_mois(&b).unwrap().starts_with("Dernier mois : octobre 2026"));
        let aucun = Filtres { chantier: Chantier::Aucun, ..Filtres::default() };
        assert!(bord::notes_de_la_declaration(&b, aucun)[0].starts_with("Pour ce chantier seulement"));
    }

    fn liste(r: &Registre, cote: Cote, filtres: listes::FiltresDeListe, tri: listes::Tri) -> Vec<i64> {
        let l = listes::calculer(r, &listes::Preparation::de(r, cote), &Filtres::default(), &filtres, tri, AUJ);
        l.lignes.iter().map(|&i| r.factures[i].id).collect()
    }

    #[test]
    fn manque_208_un_montant_nul_ne_trouve_pas_les_factures_sans_total() {
        let mut f = fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", None);
        (f.numero, f.tiers) = ("X".into(), "Y".into());
        let q = listes::FiltresDeListe { recherche: "0,00".into(), ..listes::FiltresDeListe::default() };
        assert!(liste(&reg(vec![f]), Cote::Fournisseur, q, listes::Tri::default()).is_empty());
    }

    #[test]
    fn manque_211_218_la_frappe_et_le_premier_clic() {
        assert_eq!(listes::montant_tape("1234 567,00"), None);
        let total = listes::Tri { colonne: listes::Colonne::Total, descendant: true };
        assert_eq!(total.clic(listes::Colonne::Date), listes::Tri { colonne: listes::Colonne::Date, descendant: true });
    }

    #[test]
    fn manque_221_l_ordre_naturel_ote_les_zeros_de_tete() {
        let mut a = fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(100));
        a.numero = "FF 009".into();
        let mut b = fac(2, Cote::Fournisseur, "ENVOYEE", "2026-10-02", Some(100));
        b.numero = "FF 10".into();
        let tri = listes::Tri { colonne: listes::Colonne::Numero, descendant: false };
        assert_eq!(liste(&reg(vec![a, b]), Cote::Fournisseur, listes::FiltresDeListe::default(), tri), [1, 2]);
    }

    #[test]
    fn manque_233_la_section_se_cherche() {
        let mut f = fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(100));
        f.section = Some("2. Fondation".into());
        let q = listes::FiltresDeListe { recherche: "fondation".into(), ..listes::FiltresDeListe::default() };
        assert_eq!(liste(&reg(vec![f]), Cote::Fournisseur, q, listes::Tri::default()), [1]);
    }

    #[test]
    fn manque_252_les_encaissees_se_rangent_par_leur_paiement() {
        let mut a = fac(1, Cote::Client, "PAYEE", "2026-08-01", Some(100));
        (a.paye, a.solde, a.paiement) = (Some(100), Some(0), Some("2026-10-01".into()));
        let mut b = fac(2, Cote::Client, "PAYEE", "2026-09-01", Some(100));
        (b.paye, b.solde, b.paiement) = (Some(100), Some(0), Some("2026-09-15".into()));
        let tri = listes::Tri { colonne: listes::Colonne::Paiement, descendant: false };
        assert_eq!(liste(&reg(vec![a, b]), Cote::Client, listes::FiltresDeListe::default(), tri), [2, 1]);
    }

    #[test]
    fn manque_308_la_part_d_un_fournisseur_en_credit_ne_se_dit_pas() {
        let mut avoir = fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(5_000));
        avoir.type_document = "AVOIR".into();
        let r = reg(vec![avoir, fac(2, Cote::Fournisseur, "ENVOYEE", "2026-10-02", Some(20_000))]);
        let s = sommaires::par_fournisseur(&r, &Filtres::default(), AUJ);
        let credit = s.rangees.iter().find(|l| l.depenses.total < 0).unwrap();
        assert_eq!(s.part(credit), None);
    }

    fn changement(id: i64, qui: &str, entite: &str, eid: i64) -> Changement {
        Changement {
            id,
            quand: "2026-11-02T16:04:00Z".into(),
            qui: qui.into(),
            action: "update".into(),
            entite: entite.into(),
            entite_id: Some(eid),
            details: String::new(),
        }
    }

    #[test]
    fn manque_401_410_l_heure_24_et_un_courriel_qui_commence_par_mcp() {
        assert_eq!(historique::instant("2026-10-28T24:00:00Z"), None);
        assert_eq!(historique::qui_en_mots("mcphee@exemple.ca").0, "mcphee@exemple.ca");
    }

    #[test]
    fn manque_423_425_l_historique_nomme_la_bonne_chose_et_se_cherche() {
        let mut f = fac(12, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(100));
        f.numero_fournisseur = Some("BE-4021".into());
        let r = Registre {
            factures: vec![f],
            historique: vec![changement(2, "a@b.ca", "journal_entry", 12), changement(1, "a@b.ca", "invoice", 12)],
            ..Registre::default()
        };
        let e = historique::entrees(&r, AUJ, &|t| Some(t));
        assert_eq!((e[0].facture, e[0].sur.as_str()), (None, "Écriture de journal (id 12)"));
        assert_eq!(historique::retenues(&e, "be-4021"), [1]);
    }
}

/// QA5 du lot 13.2, passe 2 (mutation) : les essais qui manquaient. Chacun passe sur le code d'aujourd'hui et rougit
/// sous le mutant de son nom.
#[cfg(test)]
mod manques_qa2 {
    use super::*;

    const AUJ: &str = "2026-11-02";

    fn fac(id: i64, cote: Cote, statut: &str, date: &str, total: Option<Cents>) -> Facture {
        Facture {
            id,
            numero: format!("N-{id}"),
            cote,
            type_document: "FACTURE".into(),
            statut: statut.into(),
            date: Some(date.into()),
            tiers: format!("Tiers {id}"),
            sous_total: total,
            tps: total.map(|_| 0),
            tvq: total.map(|_| 0),
            total,
            paye: total.map(|_| 0),
            solde: total,
            taxe1: Some("TPS".into()),
            taxe2: Some("TVQ".into()),
            ..Facture::default()
        }
    }

    fn reg(factures: Vec<Facture>) -> Registre {
        Registre { factures, ..Registre::default() }
    }

    fn chantier(id: i64, projet: i64, nom: &str) -> Facture {
        let mut f = fac(id, Cote::Client, "ENVOYEE", "2026-10-01", Some(100));
        (f.projet_id, f.projet) = (Some(projet), Some(nom.into()));
        f
    }

    /// « École » se range parmi les E, sans accents ni casse : dans le choix d'en tête comme dans « Par chantier »
    /// (mutants 111 et 309 : le tri par `to_lowercase` la mettait après « Projet 11 »).
    #[test]
    fn manque_111_309_ecole_se_range_parmi_les_e() {
        let r = reg(vec![chantier(1, 1, "Projet 11"), chantier(2, 2, "École Saint-Jean"), chantier(3, 3, "Entrepôt")]);
        let attendus = ["École Saint-Jean", "Entrepôt", "Projet 11"];
        assert_eq!(chantiers(&r).0.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(), attendus);
        let s = sommaires::par_chantier(&r, &Filtres::default(), AUJ);
        assert_eq!(s.rangees.iter().map(|l| l.nom.as_str()).collect::<Vec<_>>(), attendus);
    }

    /// Deux chantiers de même nom se rangent par leur id, partout (mutants 112 et 311).
    #[test]
    fn manque_112_311_a_nom_egal_l_id_departage() {
        let r = reg(vec![chantier(1, 9, "Bureau"), chantier(2, 4, "Bureau")]);
        assert_eq!(chantiers(&r).0.iter().map(|c| c.0).collect::<Vec<_>>(), [4, 9]);
        let s = sommaires::par_chantier(&r, &Filtres::default(), AUJ);
        assert_eq!(
            s.rangees.iter().map(|l| l.chantier).collect::<Vec<_>>(),
            [Chantier::Projet(4), Chantier::Projet(9)]
        );
    }

    /// Les noms des taxes : un nom blanc n'en est pas un, même deux fois (mutant 113) ; à égalité, le premier dans
    /// l'ordre (mutant 114).
    #[test]
    fn manque_113_114_un_nom_blanc_et_une_egalite() {
        let f = |t1: Option<&str>, t2: Option<&str>| Facture {
            id: 1,
            taxe1: t1.map(Into::into),
            taxe2: t2.map(Into::into),
            ..Facture::default()
        };
        let blancs = vec![f(Some("TPS"), Some(" ")), f(Some("TPS"), Some("")), f(Some("TPS"), Some("TVQ"))];
        assert_eq!(noms_des_taxes(&reg(blancs)), ("TPS".to_string(), "TVQ".to_string()));
        let egalite = vec![f(Some("TPS"), None), f(Some("GST"), None)];
        assert_eq!(noms_des_taxes(&reg(egalite)).0, "GST");
    }

    /// Un payé négatif (un remboursement inscrit en moins) n'encaisse rien en moins (mutant 123) ; une base qui ne dit
    /// pas le payé encaisse son total moins son solde (mutant 124).
    #[test]
    fn manque_123_124_l_encaisse_sans_paye_ou_negatif() {
        let mut f = fac(1, Cote::Client, "ENVOYEE", "2026-10-01", Some(10_000));
        (f.paye, f.solde) = (Some(-500), Some(10_500));
        assert_eq!(f.encaisse(), 0);
        let mut g = fac(2, Cote::Client, "PARTIELLEMENT_PAYEE", "2026-10-01", Some(10_000));
        (g.paye, g.solde) = (None, Some(4_000));
        assert_eq!(g.encaisse(), 6_000);
    }

    /// « Beton Fictif » et « Béton Fictif » sont une seule barre, comme la liste qu'ouvre son clic les réunit (mutant
    /// 202 : plier la casse et les espaces sans les accents en faisait deux).
    #[test]
    fn manque_202_un_fournisseur_avec_et_sans_accent_fait_une_barre() {
        let mut a = fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(100));
        a.tiers = "Béton Fictif".into();
        let mut b = fac(2, Cote::Fournisseur, "ENVOYEE", "2026-10-02", Some(100));
        b.tiers = "Beton Fictif".into();
        let bo = bord::calculer(&reg(vec![a, b]), Filtres::default(), AUJ);
        assert_eq!((bo.fournisseurs.len(), bo.fournisseurs[0].n), (1, 2));
    }

    /// « Datées après ce mois-ci » : ni le mois en cours, qui se dessine (mutant 204), ni moins un revenu (mutant 205).
    #[test]
    fn manque_204_205_les_datees_plus_tard() {
        let r = reg(vec![
            fac(1, Cote::Fournisseur, "ENVOYEE", "2026-11-01", Some(100)),
            fac(2, Cote::Client, "ENVOYEE", "2027-01-15", Some(100)),
        ]);
        let b = bord::calculer(&r, Filtres::default(), AUJ);
        assert_eq!(b.datees_plus_tard, 1);
    }

    /// Des factures toutes datées plus tard : la fenêtre se cale sur le mois en cours, jamais vide (mutant 206).
    #[test]
    fn manque_206_toutes_datees_plus_tard() {
        let r = reg(vec![fac(1, Cote::Fournisseur, "ENVOYEE", "2027-03-15", Some(100))]);
        let b = bord::calculer(&r, Filtres::default(), AUJ);
        let dernier = b.mois.last().map(bord::Mois::cle);
        assert_eq!((b.mois.len(), dernier, b.datees_plus_tard), (1, Some("2026-11".to_string()), 1));
    }

    /// Une base qui nomme la TPS et une autre taxe (une TVP, qui ne se récupère pas) : pas d'estimation (mutant
    /// 211).
    #[test]
    fn manque_211_la_tps_et_une_tvp() {
        let mut a = fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(100));
        a.taxe2 = Some("TVP".into());
        assert!(!bord::calculer(&reg(vec![a]), Filtres::default(), AUJ).taxes_recuperables);
    }

    /// Une taxe unique (la TVH) : rien de payé, la tuile dit son seul nom (mutant 215) ; ses montants, leur somme
    /// entière (mutant 214).
    #[test]
    fn manque_214_215_une_taxe_unique() {
        let mut a = fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(10_000));
        (a.taxe1, a.taxe2) = (Some("TVH".into()), None);
        let bo = bord::calculer(&reg(vec![a.clone()]), Filtres::default(), AUJ);
        assert_eq!(bord::indicateurs_des_depenses(&bo)[2].detail, "TVH");
        let mut b = fac(2, Cote::Fournisseur, "ENVOYEE", "2026-10-02", Some(11_300));
        (b.sous_total, b.tps, b.tvq, b.taxe1, b.taxe2) =
            (Some(10_000), Some(1_000), Some(300), Some("TVH".into()), None);
        let bo = bord::calculer(&reg(vec![a, b]), Filtres::default(), AUJ);
        let avant = bord::indicateurs_des_depenses(&bo)[1].detail.clone();
        assert!(avant.starts_with("TVH 13,00"), "{avant}");
    }

    /// Des taxes nettes nulles sont « à remettre », pas « en faveur de l'entreprise » (mutant 218).
    #[test]
    fn manque_218_des_taxes_nettes_nulles() {
        let r = reg(vec![
            fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(100)),
            fac(2, Cote::Client, "ENVOYEE", "2026-10-02", Some(100)),
        ]);
        let b = bord::calculer(&r, Filtres::default(), AUJ);
        let i = bord::indicateurs_des_revenus(&b, Periode::Tout);
        assert!(i[5].detail.starts_with("à remettre"), "{}", i[5].detail);
    }

    /// La tuile « Encaissé » mène aux factures des clients même sans aucune réglée (mutant 220).
    #[test]
    fn manque_220_encaisse_sans_reglee_mene_aux_clients() {
        let r = reg(vec![fac(1, Cote::Client, "ENVOYEE", "2026-10-01", Some(100))]);
        let b = bord::calculer(&r, Filtres::default(), AUJ);
        assert_eq!(bord::indicateurs_des_revenus(&b, Periode::Tout)[1].vers, Some(bord::Vers::Clients(None)));
    }

    /// « sur 1 mois qui a des dépenses » (mutant 222).
    #[test]
    fn manque_222_un_mois_qui_a_des_depenses() {
        let r = reg(vec![fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(100))]);
        let b = bord::calculer(&r, Filtres::default(), AUJ);
        assert_eq!(bord::indicateurs_des_depenses(&b)[3].detail, "sur 1 mois qui a des dépenses");
    }

    /// Trié par paiement, une facture soldée par une note de crédit se range avec les encaissées, après les notes de
    /// crédit (mutant 305) ; « En parler à Eve » le dit (mutant 306).
    #[test]
    fn manque_305_306_une_facture_soldee() {
        let mut avoir = fac(1, Cote::Client, "ENVOYEE", "2026-10-20", Some(1_000));
        avoir.type_document = "AVOIR".into();
        let mut soldee = fac(2, Cote::Client, "PAYEE", "2026-09-01", Some(1_000));
        (soldee.paye, soldee.solde) = (Some(0), Some(0));
        let r = reg(vec![avoir, soldee.clone()]);
        let tri = listes::Tri { colonne: listes::Colonne::Paiement, descendant: false };
        let p = listes::Preparation::de(&r, Cote::Client);
        let l = listes::calculer(&r, &p, &Filtres::default(), &listes::FiltresDeListe::default(), tri, AUJ);
        assert_eq!(l.lignes.iter().map(|&i| r.factures[i].id).collect::<Vec<_>>(), [1, 2]);
        let dit = listes::parler_de(&soldee, AUJ);
        assert!(dit.contains("soldée par une note de crédit"), "{dit}");
    }

    /// Une base qui ne nomme pas ses taxes : la TPS et la TVQ d'office, au tableau de bord comme aux listes (mutant
    /// 209 : le tableau de bord n'en disait plus qu'une).
    #[test]
    fn manque_209_une_base_sans_noms_de_taxe() {
        let mut a = fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(100));
        (a.taxe1, a.taxe2) = (None, None);
        let r = reg(vec![a]);
        let attendu = ("TPS".to_string(), "TVQ".to_string());
        assert_eq!(bord::calculer(&r, Filtres::default(), AUJ).taxes, attendu);
        assert_eq!(noms_des_taxes(&r), attendu);
    }

    /// Rien de payé : la tuile « Taxes payées » dit « TPS et TVQ » (mutant 224 : « TPS ou TVQ »).
    #[test]
    fn manque_224_aucune_taxe_payee() {
        let b = bord::calculer(
            &reg(vec![fac(1, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(100))]),
            Filtres::default(),
            AUJ,
        );
        assert_eq!(bord::indicateurs_des_depenses(&b)[2].detail, "TPS et TVQ");
    }

    /// « Sans chantier » se range en dernier, même après un chantier nommé plus loin dans l'alphabet (mutant 310).
    #[test]
    fn manque_310_sans_chantier_en_dernier() {
        let r = reg(vec![chantier(1, 1, "Toiture Tremblay"), fac(2, Cote::Client, "ENVOYEE", "2026-10-01", Some(100))]);
        let s = sommaires::par_chantier(&r, &Filtres::default(), AUJ);
        assert_eq!(s.rangees.iter().map(|l| l.chantier).collect::<Vec<_>>(), [Chantier::Projet(1), Chantier::Aucun]);
    }

    /// Un pour cent de mille et plus sépare ses milliers, comme l'argent (mutant 308).
    #[test]
    fn manque_308_un_pour_cent_de_mille_et_plus() {
        assert_eq!(sommaires::pour_cent(-21_933), format!("\u{2212}2\u{202f}193,3\u{a0}%"));
    }

    /// L'historique nomme une facture reçue par le numéro du fournisseur, comme la liste (mutants 313 et 314) ; le
    /// « mode » d'une publication bancaire se dit en mots (mutant 315).
    #[test]
    fn manque_313_314_315_ce_que_dit_l_historique() {
        let mut f = fac(12, Cote::Fournisseur, "ENVOYEE", "2026-10-01", Some(100));
        (f.numero, f.numero_fournisseur, f.tiers) = ("FF-1141".into(), Some("IS-48363".into()), "Béton Fictif".into());
        assert_eq!(historique::facture_nommee(&f), "IS-48363 · Béton Fictif");
        let c = Changement {
            id: 1,
            quand: "2026-11-02T16:04:00Z".into(),
            qui: "a@b.ca".into(),
            action: "update".into(),
            entite: "invoice".into(),
            entite_id: Some(12),
            details: String::new(),
        };
        let r = Registre { factures: vec![f.clone()], historique: vec![c], ..Registre::default() };
        let e = historique::entrees(&r, AUJ, &|t| Some(t));
        let dit = historique::demande(&e[0], Some(&f));
        assert!(dit.contains("la facture IS-48363 de Béton Fictif"), "{dit}");
        assert!(historique::detail_resume(r#"{"mode":"categoriser"}"#).contains("catégorisée"));
        assert!(historique::detail_resume(r#"{"mode":"transfert"}"#).contains("transfert entre comptes"));
    }
}
