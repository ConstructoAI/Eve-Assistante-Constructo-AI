//! Le registre de la vitrine de l'essai du thème : une entreprise de construction inventée — ses cinq chantiers, ses
//! douze fournisseurs, leurs factures et celles de ses clients sur dix-huit mois autour du jour de la vitrine, et un
//! historique. Personne de vrai, rien de l'ERP : de quoi montrer chaque vue de l'onglet « Comptabilité » remplie, sans
//! une lecture. Déterministe : un même jour donne le même registre (les captures se comparent).

use super::{Cents, Changement, Cote, Facture, Lecture, Registre};
use crate::centre::{date_du_numero, numero_du_jour};

/// Les chantiers : (id, nom, client, mois de début et de fin, comptés depuis le premier des dix-huit mois).
const CHANTIERS: [(i64, &str, &str, i64, i64); 5] = [
    (131, "Entrepôt Lévis", "Groupe Fictif inc.", 0, 9),
    (133, "École Saint-Romuald", "Centre de services scolaire Modèle", 3, 15),
    (135, "Condos du Fleuve", "Habitations Exemple", 6, 17),
    (137, "Résidence Bouchard", "Famille Témoin", 10, 17),
    (139, "Garage municipal", "Ville d'Essai", 13, 17),
];

/// Les fournisseurs : (id, nom, section du bordereau de l'ERP, montant typique en dollars, rang de la phase où il
/// travaille, de 0 au début d'un chantier à 10 à la fin).
const FOURNISSEURS: [(i64, &str, &str, i64, u8); 12] = [
    (211, "Excavation Exemple", "1. Excavation et terrassement", 9_800, 0),
    (201, "Béton Fictif ltée", "2. Fondation", 14_500, 1),
    (202, "Bois Modèle inc.", "3. Structure et charpente", 18_700, 3),
    (203, "Toitures Exemple", "4. Toiture", 11_200, 4),
    (204, "Portes et Fenêtres Essai", "5. Portes et fenêtres", 8_900, 5),
    (205, "Plomberie Témoin", "6. Plomberie", 6_400, 5),
    (206, "Électricité Essai", "7. Électricité", 7_300, 6),
    (207, "Ventilation Démo", "8. Ventilation / CVAC", 5_600, 6),
    (208, "Isolation Témoin", "9. Isolation complémentaire", 4_100, 7),
    (209, "Gypse Démo", "10. Finition intérieure", 6_900, 8),
    (210, "Revêtements Exemple", "11. Finition extérieure", 7_700, 9),
    (212, "Location Outils Fictifs", "12. Divers", 1_350, 10),
];

/// Un hasard reproductible (un générateur congruentiel), pour que la vitrine ne soit pas trop régulière.
struct Hasard(u64);

impl Hasard {
    fn suivant(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// Un entier de `0..n`.
    fn jusqua(&mut self, n: u64) -> u64 {
        self.suivant() % n.max(1)
    }
}

/// Les taxes du Québec sur un sous-total, au cent, la demie loin de zéro (comme EG) : TPS 5 %, TVQ 9,975 %.
fn taxes(sous_total: Cents) -> (Cents, Cents) {
    let arrondi = |x: i128, d: i128| ((x * 2 + d) / (2 * d)) as Cents;
    (arrondi(i128::from(sous_total) * 5, 100), arrondi(i128::from(sous_total) * 9_975, 100_000))
}

/// Le premier jour du mois `k` des dix-huit, comptés depuis dix-sept mois avant celui d'`aujourdhui`.
fn premier_du_mois(aujourdhui: &str, k: i64) -> String {
    let a: i64 = aujourdhui.get(..4).and_then(|x| x.parse().ok()).unwrap_or(2026);
    let m: i64 = aujourdhui.get(5..7).and_then(|x| x.parse().ok()).unwrap_or(1);
    let n = a * 12 + (m - 1) - 17 + k;
    format!("{:04}-{:02}-01", n / 12, n % 12 + 1)
}

/// Une date `jours` après une autre.
fn plus(date: &str, jours: i64) -> String {
    numero_du_jour(date).map_or_else(|| date.to_string(), |n| date_du_numero(n + jours))
}

/// Un paiement ne tombe jamais après le jour de la vitrine : au plus tard, ce jour-là.
fn paye_le(date: String, aujourdhui: &str) -> String {
    if avant(aujourdhui, &date) { aujourdhui.to_string() } else { date }
}

fn avant(a: &str, b: &str) -> bool {
    numero_du_jour(a).zip(numero_du_jour(b)).is_some_and(|(x, y)| x < y)
}

/// Le registre de la vitrine, le jour de la vitrine pour repère (« AAAA-MM-JJ »).
pub fn registre(aujourdhui: &str) -> Registre {
    let mut h = Hasard(0x00C0_FFEE);
    let mut factures: Vec<Facture> = Vec::new();
    let mut id = 1000;
    let mut no_client = 61;
    for k in 0..18 {
        let debut = premier_du_mois(aujourdhui, k);
        for (projet_id, projet, client, de, a) in CHANTIERS {
            if k < de || k > a {
                continue;
            }
            // L'avancement du chantier, de 0 à 10 : les fournisseurs de cette phase facturent ce mois-ci.
            let phase = (((k - de) * 10) / (a - de).max(1)) as u8;
            for (fid, nom, section, typique, rang) in FOURNISSEURS {
                if rang.abs_diff(phase) > 1 || h.jusqua(3) == 0 {
                    continue;
                }
                let jour = plus(&debut, h.jusqua(27) as i64);
                if !avant(&jour, aujourdhui) {
                    continue;
                }
                let sous_total: Cents = (typique * (60 + h.jusqua(80) as i64)) + h.jusqua(100) as i64;
                let (tps, tvq) = taxes(sous_total);
                let total = sous_total + tps + tvq;
                id += 1;
                let recente = !avant(&plus(&jour, 35), aujourdhui);
                let (statut, paye, paiement) = if recente {
                    match h.jusqua(5) {
                        0 => ("BROUILLON", 0, None),
                        1 => ("PARTIELLEMENT_PAYEE", total / 2, Some(paye_le(plus(&jour, 12), aujourdhui))),
                        _ => ("ENVOYEE", 0, None),
                    }
                } else {
                    ("PAYEE", total, Some(paye_le(plus(&jour, 20 + h.jusqua(20) as i64), aujourdhui)))
                };
                factures.push(Facture {
                    id,
                    numero: format!("FF-{id}"),
                    numero_fournisseur: Some(format!(
                        "{}-{}",
                        nom.chars().take(2).collect::<String>().to_uppercase(),
                        40_000 + h.jusqua(9_999)
                    )),
                    cote: Cote::Fournisseur,
                    type_document: "FACTURE".into(),
                    statut: statut.into(),
                    echeance: Some(plus(&jour, 30)),
                    date: Some(jour),
                    paiement,
                    tiers: nom.into(),
                    tiers_id: Some(fid),
                    projet_id: Some(projet_id),
                    projet: Some(projet.into()),
                    section: Some(section.into()),
                    fiabilite: match h.jusqua(9) {
                        0 => Some("a_verifier".into()),
                        1 => None,
                        _ => Some("sur".into()),
                    },
                    sous_total: Some(sous_total),
                    tps: Some(tps),
                    tvq: Some(tvq),
                    total: Some(total),
                    paye: Some(paye),
                    solde: Some(total - paye),
                    taxe1: Some("TPS".into()),
                    taxe2: Some("TVQ".into()),
                    ..Facture::default()
                });
            }
            // Le décompte du mois au client : l'avancement facturé, au 25.
            let jour = plus(&debut, 24);
            if !avant(&jour, aujourdhui) {
                continue;
            }
            let sous_total: Cents = 2_200_000 + h.jusqua(3_800_000) as Cents;
            let (tps, tvq) = taxes(sous_total);
            let total = sous_total + tps + tvq;
            id += 1;
            no_client += 1;
            let echeance = plus(&jour, 30);
            let (statut, paye, paiement) = if avant(&plus(&jour, 45), aujourdhui) {
                ("PAYEE", total, Some(paye_le(plus(&jour, 25 + h.jusqua(15) as i64), aujourdhui)))
            } else if h.jusqua(4) == 0 {
                ("PARTIELLEMENT_PAYEE", total * 2 / 5, Some(paye_le(plus(&jour, 18), aujourdhui)))
            } else {
                ("ENVOYEE", 0, None)
            };
            let annee = &jour[..4];
            factures.push(Facture {
                id,
                numero: format!("FACT-{annee}-{no_client:05}"),
                cote: Cote::Client,
                type_document: "FACTURE".into(),
                statut: statut.into(),
                echeance: Some(echeance),
                date: Some(jour),
                paiement,
                tiers: client.into(),
                tiers_id: Some(projet_id + 1000),
                projet_id: Some(projet_id),
                projet: Some(projet.into()),
                sous_total: Some(sous_total),
                tps: Some(tps),
                tvq: Some(tvq),
                total: Some(total),
                paye: Some(paye),
                solde: Some(total - paye),
                taxe1: Some("TPS".into()),
                taxe2: Some("TVQ".into()),
                ..Facture::default()
            });
        }
    }
    // Ce qui sort de l'ordinaire, pour que chaque vue ait à le montrer : une note de crédit à un client, une facture
    // annulée, une pro forma, un brouillon, une facture sans chantier, et une sans total.
    let mois = premier_du_mois(aujourdhui, 15);
    let mut extra =
        |cote: Cote, type_document: &str, statut: &str, tiers: &str, projet: Option<(i64, &str)>, st: Cents| {
            let (tps, tvq) = taxes(st);
            id += 1;
            Facture {
                id,
                numero: format!("FACT-{}-{:05}", &mois[..4], 900 + id % 100),
                cote,
                type_document: type_document.into(),
                statut: statut.into(),
                date: Some(plus(&mois, 9)),
                echeance: Some(plus(&mois, 39)),
                tiers: tiers.into(),
                projet_id: projet.map(|p| p.0),
                projet: projet.map(|p| p.1.into()),
                sous_total: Some(st),
                tps: Some(tps),
                tvq: Some(tvq),
                total: Some(st + tps + tvq),
                paye: Some(0),
                solde: Some(st + tps + tvq),
                taxe1: Some("TPS".into()),
                taxe2: Some("TVQ".into()),
                ..Facture::default()
            }
        };
    factures.push(extra(
        Cote::Client,
        "AVOIR",
        "ENVOYEE",
        "Habitations Exemple",
        Some((135, "Condos du Fleuve")),
        450_000,
    ));
    factures.push(extra(
        Cote::Fournisseur,
        "FACTURE",
        "ANNULEE",
        "Gypse Démo",
        Some((133, "École Saint-Romuald")),
        98_000,
    ));
    factures.push(extra(
        Cote::Client,
        "PROFORMA",
        "ENVOYEE",
        "Ville d'Essai",
        Some((139, "Garage municipal")),
        1_250_000,
    ));
    factures.push(extra(
        Cote::Client,
        "FACTURE",
        "BROUILLON",
        "Famille Témoin",
        Some((137, "Résidence Bouchard")),
        640_000,
    ));
    factures.push(extra(Cote::Fournisseur, "FACTURE", "ENVOYEE", "Bureau en Gros Fictif", None, 42_350));
    let mut sans_total = extra(Cote::Fournisseur, "FACTURE", "BROUILLON", "Quincaillerie Modèle", None, 0);
    sans_total.total = None;
    sans_total.sous_total = None;
    factures.push(sans_total);
    factures.sort_by_key(|f| f.id);
    // L'historique : les derniers gestes sur ces factures, du plus récent au plus ancien, dans le vocabulaire RÉEL du
    // journal de l'ERP (relevé par l'agent de l'ERP du lot 13.2) : `update` d'une `invoice` (le changement de statut
    // que note l'écran de l'ERP, `accounting.py:2240`), `send` (l'envoi), et, par le serveur MCP (`qui` :
    // « mcp:<schéma> »), `invoice_payment` et `invoice_create` d'une `facture` (`mcp_server.py:3062`, `:2789`).
    let mut historique = Vec::new();
    let mut cid = 5_600;
    for f in factures.iter().rev().take(40) {
        cid -= 1;
        let statut = |de: &str, vers: &str| format!(r#"{{"new_statut":"{vers}","old_statut":"{de}"}}"#);
        let (action, entite, details, par_mcp) = match (f.statut.as_str(), f.cote) {
            ("PAYEE", _) => ("update", "invoice", statut("ENVOYEE", "PAYEE"), false),
            ("ANNULEE", _) => ("update", "invoice", statut("ENVOYEE", "ANNULEE"), false),
            ("PARTIELLEMENT_PAYEE", _) => {
                let paye = f.paye.unwrap_or(0);
                let montant = format!(r#"{{"mode":"Virement","montant":{}.{:02}}}"#, paye / 100, paye % 100);
                ("invoice_payment", "facture", montant, true)
            }
            ("ENVOYEE", Cote::Client) => ("send", "invoice", String::new(), false),
            _ => ("invoice_create", "facture", String::new(), true),
        };
        let quand = format!(
            "{}T{:02}:{:02}:00Z",
            f.paiement.as_deref().or(f.date.as_deref()).unwrap_or(aujourdhui),
            12 + cid % 3,
            cid % 60
        );
        let qui = if par_mcp {
            "mcp:tenant_vitrine_000000"
        } else {
            ["marie@exemple.ca", "luc@exemple.ca"][(cid % 2) as usize]
        };
        historique.push(Changement {
            id: cid,
            quand,
            qui: qui.into(),
            action: action.into(),
            entite: entite.into(),
            entite_id: Some(f.id),
            details,
        });
    }
    historique.sort_by(|a, b| b.quand.cmp(&a.quand).then(b.id.cmp(&a.id)));
    // Dans l'ERP, l'id d'une entrée du journal suit le temps : le plus récent porte le plus grand (la QA5 du lot 13.2 :
    // la vue de la vitrine n'était pas chronologique).
    let n = historique.len() as i64;
    for (k, c) in historique.iter_mut().enumerate() {
        c.id = 5_600 + n - k as i64;
    }
    Registre { au_serveur: factures.len() as u64, factures, historique, ..Registre::default() }
}

/// La lecture de la vitrine : à jour, lue ce jour-là à 11 h 30, après les changements du matin.
pub fn lecture(aujourdhui: &str) -> Lecture {
    Lecture {
        etat: "a_jour".into(),
        lu_le: format!("{aujourdhui}T11:30:00"),
        erreur: String::new(),
        registre: Some(registre(aujourdhui)),
    }
}

#[cfg(test)]
mod essais {
    use super::*;

    #[test]
    fn la_vitrine_est_remplie_et_reproductible() {
        let r = registre("2026-11-06");
        assert_eq!(r, registre("2026-11-06"), "un même jour, un même registre");
        let fournisseurs = r.factures.iter().filter(|f| f.cote == Cote::Fournisseur).count();
        let clients = r.factures.iter().filter(|f| f.cote == Cote::Client).count();
        assert!(fournisseurs >= 60 && clients >= 30, "{fournisseurs} reçues, {clients} émises");
        // Aucune facture dans l'avenir ; des taxes au cent.
        assert!(r.factures.iter().all(|f| f.date.as_deref().is_none_or(|d| d <= "2026-11-06")));
        assert!(r.factures.iter().all(|f| f.paiement.as_deref().is_none_or(|d| d <= "2026-11-06")));
        assert!(r.historique.iter().all(|c| c.quand.as_str() < "2026-11-07"), "aucun changement demain");
        assert_eq!(taxes(100_000), (5_000, 9_975));
        assert_eq!(taxes(1_000), (50, 100), "une TVQ de 99,75 cents s'arrondit à 1,00 $");
        assert!(r.factures.iter().any(|f| f.est_avoir()) && r.factures.iter().any(|f| f.total.is_none()));
        assert!(r.historique.windows(2).all(|w| w[0].quand >= w[1].quand && w[0].id > w[1].id));
    }
}
