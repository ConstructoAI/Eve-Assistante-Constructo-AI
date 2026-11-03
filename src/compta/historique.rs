//! L'historique de l'onglet « Comptabilité » : le journal d'audit de la comptabilité de l'ERP (`accounting_audit_log`,
//! lu par `comptabilite_lister_historique`), mis en mots. Son dessin : `natif\compta\historique.rs`.
//!
//! Chaque changement se dit : quand (en mots, à l'heure du poste : « aujourd'hui à 11 h 04 », « hier à 16 h 30 », « le
//! 28 oct. à 9 h »), qui (le courriel ; le système ; un agent IA par le serveur MCP), ce qui s'est passé, sur quoi (une
//! facture du registre : son numéro et son tiers) et son détail résumé (« Envoyée → Payée »). Le vocabulaire est celui
//! que l'ERP écrit (`ACTIONS`, `ENTITES`, `CLES`) ; une clé qu'il ne connaît pas se montre telle quelle, jamais perdue.
//!
//! Pur : l'instant du serveur (en UTC) passe à l'heure du poste par une fonction que donne le dessin (`en_local`), et
//! le jour arrive en argument.

use std::collections::HashMap;

use serde_json::Value;

use super::{Cote, Facture, Registre, argent, cents_de};
use crate::centre::{date_courte, date_du_numero, heure_parlee, numero_du_jour};
use crate::planning::{en_francais, titre_de_statut};

// ---- Quand ----------------------------------------------------------------------------------------------------------

/// Deux chiffres ASCII, et leur valeur.
fn deux_chiffres(t: &str) -> Option<i64> {
    let o = t.as_bytes();
    (o.len() == 2 && o.iter().all(u8::is_ascii_digit)).then(|| i64::from(o[0] - b'0') * 10 + i64::from(o[1] - b'0'))
}

/// L'instant Unix (en secondes) d'une heure du serveur, ISO 8601 : « 2026-10-28T13:04:00Z », ses fractions de seconde
/// (« …:00.123456Z »), un décalage (« +00:00 », « -0400 », « -04 »), l'espace au lieu du « T » ; sans fuseau, l'heure
/// du serveur, qui est universelle. `None` si elle ne se lit pas.
pub fn instant(texte: &str) -> Option<i64> {
    let t = texte.trim();
    let jour = numero_du_jour(t.get(..10)?)?;
    let o = t.as_bytes();
    if o.get(4) != Some(&b'-') || o.get(7) != Some(&b'-') || !matches!(o.get(10), Some(b'T' | b't' | b' ')) {
        return None;
    }
    let reste = t.get(11..)?;
    let h = deux_chiffres(reste.get(..2)?)?;
    if reste.get(2..3)? != ":" {
        return None;
    }
    let mi = deux_chiffres(reste.get(3..5)?)?;
    let (s, mut suite) = match reste.get(5..6) {
        Some(":") => (deux_chiffres(reste.get(6..8)?)?, reste.get(8..)?),
        _ => (0, reste.get(5..)?),
    };
    if let Some(f) = suite.strip_prefix(['.', ',']) {
        suite = f.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let decalage = match suite {
        "" | "Z" | "z" => 0,
        z => {
            let signe = match z.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let z = z[1..].replace(':', "");
            let (zh, zm) = match z.len() {
                2 => (deux_chiffres(&z)?, 0),
                4 => (deux_chiffres(&z[..2])?, deux_chiffres(&z[2..])?),
                _ => return None,
            };
            signe * (zh * 3600 + zm * 60)
        }
    };
    if h > 23 || mi > 59 || s > 60 {
        return None;
    }
    Some(jour * 86_400 + h * 3600 + mi * 60 + s.min(59) - decalage)
}

/// Quand, en mots, une heure locale (`local` : ses secondes civiles, comptées comme une heure universelle), au jour
/// `aujourdhui` (« AAAA-MM-JJ ») : « aujourd'hui à 11 h 04 », « hier à 16 h 30 », « demain à 9 h », « le 28 oct. à
/// 9 h », et l'année quand ce n'est pas celle d'aujourd'hui (« le 28 oct. 2025 à 9 h »).
pub fn quand_en_mots(local: i64, aujourdhui: &str) -> String {
    let jour = local.div_euclid(86_400);
    let (date, heure) = date_et_heure(local);
    match numero_du_jour(aujourdhui).map(|a| jour - a) {
        Some(0) => format!("aujourd'hui à {heure}"),
        Some(-1) => format!("hier à {heure}"),
        Some(1) => format!("demain à {heure}"),
        _ if date.get(..4) == aujourdhui.get(..4) => format!("le {} à {heure}", date_courte(&date)),
        _ => le_jour_et_l_heure(local),
    }
}

/// Une heure locale en mots, sans rapport au jour d'aujourd'hui (un rappel se lira demain) : « le 1er nov. 2026 à
/// 16 h 30 ».
pub fn le_jour_et_l_heure(local: i64) -> String {
    let (date, heure) = date_et_heure(local);
    format!("le {} {} à {heure}", date_courte(&date), date.get(..4).unwrap_or(""))
}

/// Le jour (« AAAA-MM-JJ ») et l'heure dite (« 9 h », « 16 h 30 ») d'une heure locale.
fn date_et_heure(local: i64) -> (String, String) {
    let s = local.rem_euclid(86_400);
    (date_du_numero(local.div_euclid(86_400)), heure_parlee(&format!("{:02}:{:02}", s / 3600, s / 60 % 60)))
}

// ---- Qui ------------------------------------------------------------------------------------------------------------

/// Qui a fait le changement, pour la colonne et pour une phrase (« par … ») : un courriel tel quel ; personne (le
/// système : une tâche de l'ERP) ; un agent IA par le serveur MCP de Constructo AI, qui signe `mcp:<le schéma de
/// l'entreprise>` (`mcp_server.py`, `_log_compta_mcp`).
pub fn qui_en_mots(qui: &str) -> (String, String) {
    let q = qui.trim();
    if q.is_empty() {
        ("Système".into(), "par le système".into())
    } else if q.eq_ignore_ascii_case("mcp") || q.get(..4).is_some_and(|d| d.eq_ignore_ascii_case("mcp:")) {
        ("Agent IA (MCP)".into(), "par un agent IA (MCP)".into())
    } else {
        (q.to_string(), format!("par {q}"))
    }
}

// ---- Quoi -----------------------------------------------------------------------------------------------------------

/// Les actions du journal (`accounting_audit_log.action`) que l'ERP écrit, et leurs mots : `accounting.py`
/// (`_log_accounting_action`), `banque.py` (`_tracer`), `tax_us_1099.py`, et le serveur MCP (`mcp_server.py`,
/// `_log_compta_mcp`). Une action générique (« create », « update »…) se dit d'après ce qu'elle touche
/// ([`ACTIONS_SUR`]) avant de se dire seule.
const ACTIONS: &[(&str, &str)] = &[
    // Le plan comptable.
    ("create_account", "Compte créé"),
    ("update_account", "Compte modifié"),
    ("soft_delete_account", "Compte désactivé"),
    ("hard_delete_account", "Compte supprimé"),
    // Les factures.
    ("invoice_create", "Facture créée"),
    ("send", "Facture envoyée"),
    ("pay", "Paiement reçu"),
    ("settle", "Fournisseur payé"),
    ("invoice_payment", "Paiement enregistré"),
    ("reverse", "Écritures contre-passées"),
    ("auto_generate", "Facture produite par une récurrence"),
    ("send_reminder", "Rappel de paiement envoyé"),
    // Les écritures.
    ("journal_entry_create", "Écriture créée"),
    ("cleanup_orphan", "Écriture orpheline supprimée"),
    // La banque.
    ("bank_account_create", "Compte bancaire créé"),
    ("bank_account_update", "Compte bancaire modifié"),
    ("bank_account_archive", "Compte bancaire archivé"),
    ("bank_upload", "Relevé déposé"),
    ("bank_import", "Relevé importé"),
    ("bank_publish", "Opération bancaire publiée"),
    ("bank_unpublish", "Publication annulée"),
    ("bank_exclude", "Opération exclue du rapprochement"),
    ("bank_restore", "Opération rétablie"),
    // Les feuillets des États-Unis.
    ("1099_nec_generate", "Feuillet 1099-NEC produit"),
    // Les verbes génériques, quand ce qu'ils touchent n'est pas dans `ACTIONS_SUR`.
    ("create", "Création"),
    ("update", "Modification"),
    ("delete", "Suppression"),
    ("validate", "Validation"),
    ("close", "Fermeture"),
    ("reopen", "Réouverture"),
    ("cancel", "Annulation"),
];

/// Les verbes génériques de l'ERP et ce qu'ils touchent (`entity_type`) : leurs mots.
const ACTIONS_SUR: &[(&str, &str, &str)] = &[
    ("create", "journal_entry", "Écriture créée"),
    ("validate", "journal_entry", "Écriture validée"),
    ("create", "accounting_period", "Période créée"),
    ("close", "accounting_period", "Période fermée"),
    ("reopen", "accounting_period", "Période rouverte"),
    ("create", "credit_note", "Note de crédit créée"),
    ("update", "invoice", "Facture modifiée"),
    ("delete", "invoice", "Facture supprimée"),
    ("create", "recurring_invoice", "Facture récurrente créée"),
    ("update", "recurring_invoice", "Facture récurrente modifiée"),
    ("cancel", "recurring_invoice", "Facture récurrente annulée"),
];

/// Ce que touche un changement (`accounting_audit_log.entity_type`), en mots.
const ENTITES: &[(&str, &str)] = &[
    ("invoice", "Facture"),
    ("facture", "Facture"),
    ("credit_note", "Note de crédit"),
    ("recurring_invoice", "Facture récurrente"),
    ("journal_entry", "Écriture de journal"),
    ("chart_of_account", "Compte du plan comptable"),
    ("accounting_period", "Période comptable"),
    ("banque_compte", "Compte bancaire"),
    ("banque_import", "Relevé bancaire"),
    ("banque_operation", "Opération bancaire"),
    ("tax_us_1099_filing", "Feuillet 1099-NEC"),
];

/// Ce qui s'est passé, en mots : l'action sur ce qu'elle touche, sinon l'action seule ; une action inconnue, telle que
/// l'ERP l'écrit.
pub fn action_en_mots(action: &str, entite: &str) -> String {
    let (a, e) = (action.trim(), entite.trim());
    let sur = ACTIONS_SUR.iter().find(|(x, y, _)| x.eq_ignore_ascii_case(a) && y.eq_ignore_ascii_case(e));
    if let Some((_, _, mots)) = sur {
        return (*mots).to_string();
    }
    match ACTIONS.iter().find(|(x, _)| x.eq_ignore_ascii_case(a)) {
        Some((_, mots)) => (*mots).to_string(),
        None if a.is_empty() => "Changement".into(),
        None => a.to_string(),
    }
}

/// Ce que touche un changement, en mots ; une sorte inconnue, telle que l'ERP l'écrit.
pub fn entite_en_mots(entite: &str) -> String {
    let e = entite.trim();
    match ENTITES.iter().find(|(x, _)| x.eq_ignore_ascii_case(e)) {
        Some((_, mots)) => (*mots).to_string(),
        None => e.to_string(),
    }
}

/// Le changement touche-t-il une facture du registre (une facture, ou une note de crédit, qui en est une) ?
pub fn touche_une_facture(entite: &str) -> bool {
    ["invoice", "facture", "credit_note"].iter().any(|x| x.eq_ignore_ascii_case(entite.trim()))
}

// ---- Le détail ------------------------------------------------------------------------------------------------------

/// Comment se dit la valeur d'une clé du détail.
#[derive(Clone, Copy, Debug)]
enum Forme {
    /// Un montant : « 1 149,75 $ ».
    Argent,
    /// Un statut de l'ERP : « Payée ».
    Statut,
    /// Un mot de l'ERP, accentué : « VENTES » → « Ventes ».
    Mot,
    /// Une date : « 28 oct. 2026 ».
    Date,
    /// Un nombre et son mot, au singulier ou au pluriel (« 1 ligne », « 3 lignes ») ; zéro ne se dit pas.
    Compte(&'static str, &'static str),
    /// Un texte, ou un nombre, tel quel.
    Texte,
    /// Vrai : le premier mot ; faux : le second (vide : rien).
    OuiNon(&'static str, &'static str),
    /// Un mode de paiement : « chèque ».
    Mode,
    /// Une liste (des champs) : « nom, fréquence ».
    Liste,
    /// Rien : une clé technique, qui ne dit rien à la personne (un id interne, une taille de fichier, une clé déjà
    /// dite par un changement de statut).
    Rien,
}

/// Les clés du détail que l'ERP écrit (`details` de `_log_accounting_action`, de `_tracer` et de `_log_compta_mcp`),
/// dans l'ordre où elles se disent, leur libellé (vide : la valeur seule) et leur forme. Une clé qui n'y est pas se dit
/// telle quelle (« clé : valeur »), après elles.
const CLES: &[(&str, &str, Forme)] = &[
    // Ce qui nomme la chose.
    ("numero", "", Forme::Texte),
    ("numero_avoir", "", Forme::Texte),
    ("code", "", Forme::Texte),
    ("nom", "", Forme::Texte),
    ("libelle", "", Forme::Texte),
    ("fichier", "", Forme::Texte),
    ("compte_code", "compte", Forme::Texte),
    ("numero_ecriture", "écriture", Forme::Texte),
    // L'argent.
    ("montant", "", Forme::Argent),
    ("montant_ttc", "", Forme::Argent),
    ("montant_ht", "avant taxes", Forme::Argent),
    ("montant_total", "total", Forme::Argent),
    ("total", "total", Forme::Argent),
    ("total_debit", "débit", Forme::Argent),
    ("total_credit", "crédit", Forme::Argent),
    ("total_brut", "brut", Forme::Argent),
    ("total_net", "net", Forme::Argent),
    ("total_deductions", "retenues", Forme::Argent),
    ("total_charges_employeur", "charges de l'employeur", Forme::Argent),
    ("box1", "case 1", Forme::Argent),
    ("box4", "case 4", Forme::Argent),
    ("new_solde", "solde", Forme::Argent),
    // Le paiement, l'envoi.
    ("mode_paiement", "", Forme::Mode),
    ("methode", "", Forme::Mode),
    ("mode", "", Forme::Mode),
    ("reference", "réf.", Forme::Texte),
    ("date_paiement", "payée le", Forme::Date),
    ("to_email", "à", Forme::Texte),
    ("cc", "cc", Forme::Texte),
    ("bcc", "cci", Forme::Texte),
    // Le reste.
    ("statut", "", Forme::Statut),
    ("new_statut", "statut :", Forme::Statut),
    ("type_journal", "journal", Forme::Mot),
    ("type", "type :", Forme::Mot),
    ("type_compte", "type :", Forme::Mot),
    ("annee_fiscale", "exercice", Forme::Texte),
    ("periode", "période", Forme::Texte),
    ("tax_year", "année", Forme::Texte),
    ("frequence", "", Forme::Mot),
    ("date_debut", "dès le", Forme::Date),
    ("niveau", "rappel", Forme::Texte),
    ("auto", "", Forme::OuiNon("automatique", "")),
    ("occurrence_no", "occurrence", Forme::Texte),
    ("next_date", "prochaine le", Forme::Date),
    ("terminated", "", Forme::OuiNon("la dernière", "")),
    ("rappels_actifs", "", Forme::OuiNon("rappels activés", "rappels désactivés")),
    ("lines", "", Forme::Compte("ligne", "lignes")),
    ("lignes", "", Forme::Compte("ligne", "lignes")),
    ("nb_lignes", "", Forme::Compte("ligne", "lignes")),
    ("nb_lines_deleted", "", Forme::Compte("ligne retirée", "lignes retirées")),
    ("reversed_entries", "", Forme::Compte("écriture contre-passée", "écritures contre-passées")),
    ("usages_count", "", Forme::Compte("usage", "usages")),
    ("inserees", "", Forme::Compte("ajoutée", "ajoutées")),
    ("doublons", "", Forme::Compte("doublon", "doublons")),
    ("rejetees", "", Forme::Compte("rejetée", "rejetées")),
    ("operations_publiees", "", Forme::Compte("opération publiée", "opérations publiées")),
    ("fields_updated", "champs :", Forme::Liste),
    ("fields", "champs :", Forme::Liste),
    ("champs", "champs :", Forme::Liste),
    ("raison", "raison :", Forme::Texte),
    ("motif", "motif :", Forme::Texte),
    ("erreur", "erreur :", Forme::Texte),
    ("parent_code", "parent :", Forme::Texte),
    // Ce qui ne dit rien à la personne, ou se dit déjà ailleurs.
    ("de", "", Forme::Rien),
    ("vers", "", Forme::Rien),
    ("old_statut", "", Forme::Rien),
    ("previous_statut", "", Forme::Rien),
    ("journal_entry_id", "", Forme::Rien),
    ("je_id", "", Forme::Rien),
    ("pdf_size_bytes", "", Forme::Rien),
    ("octets", "", Forme::Rien),
    ("ratio", "", Forme::Rien),
    ("source_type", "", Forme::Rien),
    ("source_id", "", Forme::Rien),
    ("from_recurring_id", "", Forme::Rien),
    ("facture_origine_id", "", Forme::Rien),
    ("compte_tresorerie", "", Forme::Rien),
    ("type_destinataire", "", Forme::Rien),
    ("miroir", "", Forme::Rien),
    ("contre_passation", "", Forme::Rien),
    ("operations", "", Forme::Rien),
    ("contractor_id", "", Forme::Rien),
    ("has_w9", "", Forme::Rien),
    ("output_format", "", Forme::Rien),
    ("client", "", Forme::Rien),
    ("interval_count", "", Forme::Rien),
    ("solde_normal", "", Forme::Rien),
    ("journal_lines_count", "", Forme::Rien),
    ("grand_livre_count", "", Forme::Rien),
    ("balanced", "", Forme::Rien),
];

/// Les changements de statut que porte un détail : (avant, après).
const STATUTS: [(&str, &str); 4] =
    [("de", "vers"), ("old_statut", "new_statut"), ("previous_statut", "new_statut"), ("statut_avant", "statut_apres")];

/// Les modes de paiement de l'ERP, en mots.
const MODES: &[(&str, &str)] = &[
    ("cheque", "chèque"),
    ("virement", "virement"),
    ("virement_interac", "virement Interac"),
    ("interac", "virement Interac"),
    ("comptant", "comptant"),
    ("especes", "comptant"),
    ("cash", "comptant"),
    ("carte", "carte de crédit"),
    ("carte_credit", "carte de crédit"),
    ("credit_card", "carte de crédit"),
    ("debit", "carte de débit"),
    ("depot_direct", "dépôt direct"),
    ("eft", "dépôt direct"),
    ("tef", "dépôt direct"),
    // Le `mode` d'une publication bancaire n'est pas un mode de paiement (`banque.py:152`) : catégoriser l'opération,
    // ou la passer en transfert entre deux comptes (la QA5 du lot 13.2 : « publiée · categoriser »).
    ("categoriser", "catégorisée"),
    ("transfert", "transfert entre comptes"),
];

/// Une valeur JSON en texte : une chaîne sans ses blancs, un nombre tel qu'écrit, vrai ou faux en mots ; `None` pour
/// rien (nul, vide).
fn en_texte(v: &Value) -> Option<String> {
    let t = match v {
        Value::Null => return None,
        Value::String(s) => s.trim().to_string(),
        Value::Bool(b) => (if *b { "oui" } else { "non" }).to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(l) => l.iter().filter_map(en_texte).collect::<Vec<_>>().join(", "),
        Value::Object(_) => v.to_string(),
    };
    (!t.is_empty()).then_some(t)
}

/// Une date du détail (« 2026-10-28 », ou un instant) : « 28 oct. 2026 » ; ce qui n'en est pas une, tel quel.
fn date_dite(t: &str) -> String {
    match t.get(..10).filter(|d| numero_du_jour(d).is_some()) {
        Some(d) => format!("{} {}", date_courte(d), &d[..4]),
        None => t.to_string(),
    }
}

/// La valeur d'une clé du détail dans sa forme ; `None` : elle ne se dit pas.
fn valeur_dite(v: &Value, forme: Forme) -> Option<String> {
    match forme {
        Forme::Rien => None,
        Forme::OuiNon(oui, non) => {
            let vrai = match v {
                Value::Bool(b) => *b,
                Value::Null => return None,
                autre => matches!(en_texte(autre)?.to_lowercase().as_str(), "true" | "oui" | "1"),
            };
            let mot = if vrai { oui } else { non };
            (!mot.is_empty()).then(|| mot.to_string())
        }
        Forme::Compte(un, plusieurs) => {
            let n = match v {
                Value::Number(n) => n.as_i64()?,
                Value::String(s) => s.trim().parse().ok()?,
                _ => return None,
            };
            (n != 0).then(|| format!("{n} {}", if n.abs() > 1 { plusieurs } else { un }))
        }
        _ => {
            let t = en_texte(v)?;
            Some(match forme {
                Forme::Argent => cents_de(&t).map_or(t, argent),
                Forme::Statut => titre_de_statut(&t),
                Forme::Mot => en_francais(&t),
                Forme::Date => date_dite(&t),
                Forme::Mode => {
                    let n = t.to_lowercase().replace(['-', ' '], "_");
                    MODES
                        .iter()
                        .find(|(x, _)| *x == n)
                        .map_or_else(|| t.to_lowercase().replace('_', " "), |m| m.1.into())
                }
                _ => t,
            })
        }
    }
}

/// Une clé brute en libellé : ses `_` en espaces (« solde_normal » → « solde normal »).
fn libelle_brut(cle: &str) -> String {
    cle.trim().replace('_', " ")
}

/// Le détail d'un changement (le JSON compact du journal, coupé à 300 caractères par le serveur), résumé en une ligne :
/// un changement de statut d'abord (« Envoyée → Payée »), puis ses clés connues dans l'ordre de [`CLES`], puis les
/// autres telles quelles. Un détail qui ne se lit pas (coupé au milieu) se montre tel quel, sans ses accolades.
pub fn detail_resume(details: &str) -> String {
    let d = details.trim();
    if d.is_empty() || d == "{}" || d == "null" {
        return String::new();
    }
    let objet = match serde_json::from_str::<Value>(d) {
        Ok(Value::Object(o)) => o,
        Ok(autre) => return en_texte(&autre).unwrap_or_default(),
        Err(_) => {
            // Ses paires telles qu'elles se lisent encore (« "raison":"Erreur de quantit » → « raison : Erreur de
            // quantit… ») ; les points de suspension disent qu'il est coupé.
            let brut = d.trim_start_matches('{').trim_end_matches('}');
            let brut = brut
                .replace("\",\"", " · ")
                .replace("\":\"", " : ")
                .replace("\":", " : ")
                .replace(",\"", " · ")
                .replace('"', "");
            let coupe = if d.ends_with('}') { "" } else { "…" };
            return format!("{}{coupe}", brut.trim());
        }
    };
    let mut morceaux: Vec<String> = Vec::new();
    // Un changement de statut.
    let mut vers_dit = false;
    for (de, vers) in STATUTS {
        let (Some(a), Some(b)) = (objet.get(de).and_then(en_texte), objet.get(vers).and_then(en_texte)) else {
            continue;
        };
        if !vers_dit {
            morceaux.push(format!("{} \u{2192} {}", titre_de_statut(&a), titre_de_statut(&b)));
            vers_dit = true;
        }
    }
    // Les champs changés, avant et après (« nom_avant », « nom_apres »).
    let mut paires: Vec<&String> = Vec::new();
    for cle in objet.keys() {
        let Some(racine) = cle.strip_suffix("_avant") else { continue };
        let apres = format!("{racine}_apres");
        if objet.contains_key(&apres) && !racine.eq_ignore_ascii_case("statut") {
            let a = objet.get(cle).and_then(en_texte).map_or_else(|| "(vide)".into(), |t| en_francais(&t));
            let b = objet.get(&apres).and_then(en_texte).map_or_else(|| "(vide)".into(), |t| en_francais(&t));
            morceaux.push(format!("{} : {a} \u{2192} {b}", libelle_brut(racine)));
            paires.push(cle);
        }
    }
    let dans_une_paire = |cle: &str| {
        paires.iter().any(|p| *p == cle)
            || cle.strip_suffix("_apres").is_some_and(|r| paires.iter().any(|p| p.strip_suffix("_avant") == Some(r)))
            || (cle.starts_with("statut_") && vers_dit)
    };
    // Les clés connues, dans leur ordre ; un statut d'arrivée déjà dit ne se redit pas.
    for (cle, libelle, forme) in CLES {
        let Some(v) = objet.get(*cle) else { continue };
        if (vers_dit && *cle == "new_statut") || dans_une_paire(cle) {
            continue;
        }
        if let Some(t) = valeur_dite(v, *forme) {
            morceaux.push(if libelle.is_empty() { t } else { format!("{libelle} {t}") });
        }
    }
    // Les autres, telles quelles : jamais perdues.
    for (cle, v) in &objet {
        if CLES.iter().any(|(c, _, _)| c == cle) || dans_une_paire(cle) {
            continue;
        }
        if let Some(t) = en_texte(v) {
            morceaux.push(format!("{} : {t}", libelle_brut(cle)));
        }
    }
    morceaux.join(" · ")
}

// ---- Les entrées ----------------------------------------------------------------------------------------------------

/// Une entrée de l'historique, mise en mots.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entree {
    pub id: i64,
    /// Quand, en mots ; l'instant tel quel s'il ne se lit pas. Et son heure locale (ses secondes civiles), pour la dire
    /// autrement (un rappel).
    pub quand: String,
    pub local: Option<i64>,
    /// Qui, pour la colonne ; et pour une phrase (« par marie@exemple.ca »).
    pub qui: String,
    pub par: String,
    /// Ce qui s'est passé.
    pub quoi: String,
    /// Sur quoi : une facture du registre (son numéro et son tiers), sinon ce qu'il touche et son id.
    pub sur: String,
    /// La facture touchée : son rang dans `Registre::factures`, si la lecture l'a.
    pub facture: Option<usize>,
    /// Le détail résumé.
    pub detail: String,
    /// Tout ce qui précède, normalisé (minuscules, sans accents ni ponctuation) : la recherche s'y fait.
    pub cherche: String,
}

/// Une facture du registre, nommée : son numéro et son tiers (« FACT-2026-00104 · Béton Fictif ltée »).
pub fn facture_nommee(f: &Facture) -> String {
    // Le numéro que montre la liste (`listes::numero_dit` : celui du fournisseur pour une facture reçue) : une même
    // facture ne porte plus deux numéros d'une vue à l'autre (la QA5 : « FF-1141 » ici, « IS-48363 » dans la liste).
    let dit = super::listes::numero_dit(f);
    let numero = if dit.is_empty() { format!("Facture (id {})", f.id) } else { dit.to_string() };
    if f.tiers.trim().is_empty() { numero } else { format!("{numero} · {}", f.tiers.trim()) }
}

/// Les entrées de l'historique du registre, dans l'ordre du journal (du plus récent au plus ancien) : `aujourdhui`
/// (« AAAA-MM-JJ ») pour dire quand ; `en_local` : l'heure locale du poste (ses secondes civiles) d'un instant Unix,
/// l'heure d'été de ce jour-là comprise.
pub fn entrees(r: &Registre, aujourdhui: &str, en_local: &dyn Fn(i64) -> Option<i64>) -> Vec<Entree> {
    let par_id: HashMap<i64, usize> = r.factures.iter().enumerate().map(|(k, f)| (f.id, k)).collect();
    r.historique
        .iter()
        .map(|c| {
            let local = instant(&c.quand).and_then(en_local);
            let quand = match local {
                Some(l) => quand_en_mots(l, aujourdhui),
                None if c.quand.trim().is_empty() => "Date inconnue".into(),
                None => c.quand.trim().to_string(),
            };
            let (qui, par) = qui_en_mots(&c.qui);
            let facture =
                if touche_une_facture(&c.entite) { c.entite_id.and_then(|id| par_id.get(&id).copied()) } else { None };
            let sur = match (facture.map(|k| &r.factures[k]), c.entite_id) {
                (Some(f), _) => facture_nommee(f),
                (None, Some(id)) => format!("{} (id {id})", entite_en_mots(&c.entite)),
                (None, None) => entite_en_mots(&c.entite),
            };
            let quoi = action_en_mots(&c.action, &c.entite);
            let detail = detail_resume(&c.details);
            let fournisseur = facture.and_then(|k| r.factures[k].numero_fournisseur.as_deref()).unwrap_or("");
            let cherche = eve::parole::normaliser(&format!("{quand} {qui} {quoi} {sur} {fournisseur} {detail}"));
            Entree { id: c.id, quand, local, qui, par, quoi, sur, facture, detail, cherche }
        })
        .collect()
}

/// Les entrées que retient une recherche (leurs rangs) : chacun de ses mots, sans la casse ni les accents, quelque
/// part dans l'entrée ; une recherche vide les retient toutes.
pub fn retenues(entrees: &[Entree], recherche: &str) -> Vec<usize> {
    let mots: Vec<String> =
        eve::parole::normaliser(recherche).split(' ').filter(|m| !m.is_empty()).map(String::from).collect();
    entrees
        .iter()
        .enumerate()
        .filter(|(_, e)| mots.iter().all(|m| e.cherche.contains(m.as_str())))
        .map(|(k, _)| k)
        .collect()
}

// ---- Ce qu'on demande à Eve -----------------------------------------------------------------------------------------

/// Une facture dans une phrase : « la facture FACT-2026-00104 à Groupe Fictif inc., 1 149,75 $ (id 104 dans
/// Constructo AI) » — « de » un fournisseur, « à » un client ; une note de crédit se dit ainsi.
fn facture_dite(f: &Facture) -> String {
    let sorte = if f.est_avoir() { "la note de crédit" } else { "la facture" };
    let dit = super::listes::numero_dit(f);
    let numero = if dit.is_empty() { String::new() } else { format!(" {dit}") };
    let tiers = match (f.tiers.trim(), f.cote) {
        ("", _) => String::new(),
        (t, Cote::Fournisseur) => format!(" de {t}"),
        (t, Cote::Client) => format!(" à {t}"),
    };
    let montant = f.total.map_or_else(|| "sans total".to_string(), argent);
    format!("{sorte}{numero}{tiers}, {montant} (id {} dans Constructo AI)", f.id)
}

/// Le détail d'une entrée au bout d'une phrase : « — Brouillon → Envoyée » ; rien sans détail.
fn detail_en_queue(e: &Entree) -> String {
    if e.detail.is_empty() { String::new() } else { format!(" \u{2014} {}", e.detail) }
}

/// « En parler à Eve » d'une entrée : la facture qu'elle touche (son numéro, son tiers, son montant, son id dans
/// l'ERP), sinon ce qu'elle touche ; et ce que dit le journal (« Facture envoyée le 28 oct. à 9 h par
/// marie@exemple.ca — Brouillon → Envoyée »). Claude sait la date du jour : « aujourd'hui », « hier » lui suffisent.
pub fn demande(e: &Entree, f: Option<&Facture>) -> String {
    let journal = format!("{} {} {}", e.quoi, e.quand, e.par);
    match f {
        Some(f) => format!(
            "Parlons de {}, vue à l'historique de la comptabilité. Au journal : {journal}{}.",
            facture_dite(f),
            detail_en_queue(e)
        ),
        None => format!(
            "Parlons de ce changement, vu à l'historique de la comptabilité : {journal}, sur : {}{}.",
            e.sur,
            detail_en_queue(e)
        ),
    }
}

/// « Me le rappeler demain » d'une entrée : ce que dira le rappel, demain — le jour en entier, sans « hier ».
pub fn rappel(e: &Entree, f: Option<&Facture>) -> String {
    let le = e.local.map_or_else(|| e.quand.clone(), le_jour_et_l_heure);
    match f {
        Some(f) => {
            let montant = f.total.map_or_else(|| "sans total".to_string(), argent);
            format!("Comptabilité : {}, {montant} ({} {le})", facture_nommee(f), e.quoi.to_lowercase())
        }
        None => format!("Comptabilité : {}, {} ({le})", e.quoi, e.sur),
    }
}

#[cfg(test)]
mod essais {
    use super::super::Changement;
    use super::*;

    /// L'heure de Montréal d'un instant Unix, écrite à la main pour les essais : l'heure normale de l'Est (UTC−5), et
    /// l'heure avancée (UTC−4) du 8 mars 2026 à 2 h au 1er novembre 2026 à 2 h (de 7 h à 6 h, en temps universel).
    fn montreal(unix: i64) -> Option<i64> {
        // Le 8 mars 2026 à 7 h UTC : 20 520 jours depuis 1970, et 25 200 secondes ; le 1er novembre à 6 h UTC : 20 758
        // jours, et 21 600 secondes.
        let ete = 1_772_953_200..1_793_512_800;
        Some(unix - if ete.contains(&unix) { 4 * 3600 } else { 5 * 3600 })
    }

    #[test]
    fn les_instants_du_serveur() {
        // Le 28 octobre 2026, 13 h 04 UTC : 20 754 jours depuis 1970 (le 1er janvier 2026 en est le 20 454e, plus
        // 300), et 47 040 secondes.
        let t = 1_793_192_640;
        assert_eq!(instant("2026-10-28T13:04:00Z"), Some(t));
        assert_eq!(instant("2026-10-28T13:04:00.123456Z"), Some(t), "les fractions de seconde");
        assert_eq!(instant("2026-10-28 13:04:00"), Some(t), "sans fuseau : l'heure du serveur, universelle");
        assert_eq!(instant("2026-10-28T13:04Z"), Some(t), "sans les secondes");
        assert_eq!(instant("2026-10-28T09:04:00-04:00"), Some(t));
        assert_eq!(instant("2026-10-28T09:04:00-0400"), Some(t));
        assert_eq!(instant("2026-10-28T14:04:00+01"), Some(t));
        assert_eq!(instant("2026-10-28T13:04:07+00:00"), Some(t + 7));
        for faux in [
            "", "hier", "2026-10-28", "2026-10-28T25:00:00Z", "2026-10-28T13:4:00Z", "2026-13-01T00:00:00Z",
            "2026-10-28T13:04:00 UTC", "2026-10-28T13:04:00+5", "2026/10/28T13:04:00Z", "2026-10-28X13:04:00Z",
        ] {
            assert_eq!(instant(faux), None, "{faux:?}");
        }
    }

    #[test]
    fn quand_en_mots_a_l_heure_de_montreal() {
        let dit = |iso: &str, aujourdhui: &str| quand_en_mots(montreal(instant(iso).unwrap()).unwrap(), aujourdhui);
        // Le 2 novembre 2026 (l'heure normale de l'Est) : 16 h 04 UTC est 11 h 04 à Montréal.
        assert_eq!(dit("2026-11-02T16:04:00Z", "2026-11-02"), "aujourd'hui à 11 h 04");
        // L'heure avancée du 1er novembre au matin, l'heure normale l'après-midi : 21 h 30 UTC est 16 h 30.
        assert_eq!(dit("2026-11-01T21:30:00Z", "2026-11-02"), "hier à 16 h 30");
        // Le 28 octobre, encore à l'heure avancée : 13 h UTC est 9 h, sans minutes.
        assert_eq!(dit("2026-10-28T13:00:00Z", "2026-11-02"), "le 28 oct. à 9 h");
        // L'instant qui change de jour : le 29 octobre à 2 h 30 UTC est encore le 28 à Montréal (22 h 30).
        assert_eq!(dit("2026-10-29T02:30:00Z", "2026-10-29"), "hier à 22 h 30");
        assert_eq!(dit("2026-11-03T04:59:00Z", "2026-11-02"), "aujourd'hui à 23 h 59", "UTC−5 en novembre");
        assert_eq!(dit("2026-11-03T05:00:00Z", "2026-11-02"), "demain à 0 h");
        // Une autre année, et le premier du mois.
        assert_eq!(dit("2025-12-01T14:15:00Z", "2026-11-02"), "le 1er déc. 2025 à 9 h 15");
        assert_eq!(dit("2026-01-01T04:30:00Z", "2026-01-02"), "le 31 déc. 2025 à 23 h 30", "le jour de l'an à Londres");
    }

    #[test]
    fn qui_et_quoi_en_mots() {
        assert_eq!(qui_en_mots(""), ("Système".into(), "par le système".into()));
        assert_eq!(qui_en_mots(" marie@exemple.ca "), ("marie@exemple.ca".into(), "par marie@exemple.ca".into()));
        assert_eq!(qui_en_mots("mcp:tenant_exemple_123").0, "Agent IA (MCP)");
        assert_eq!(qui_en_mots("MCP").1, "par un agent IA (MCP)");
        assert_eq!(action_en_mots("update", "invoice"), "Facture modifiée");
        assert_eq!(action_en_mots("create", "credit_note"), "Note de crédit créée");
        assert_eq!(action_en_mots("create", "journal_entry"), "Écriture créée");
        assert_eq!(action_en_mots("settle", "invoice"), "Fournisseur payé");
        assert_eq!(action_en_mots("bank_publish", "banque_operation"), "Opération bancaire publiée");
        assert_eq!(action_en_mots("create", "quelque_chose"), "Création", "un verbe générique sur l'inconnu");
        // Une clé inconnue : telle quelle, jamais perdue.
        assert_eq!(action_en_mots("archive_everything", "invoice"), "archive_everything");
        assert_eq!(entite_en_mots("chart_of_account"), "Compte du plan comptable");
        assert_eq!(entite_en_mots("widget_x"), "widget_x");
        assert!(
            touche_une_facture("INVOICE") && touche_une_facture("credit_note") && !touche_une_facture("journal_entry")
        );
    }

    #[test]
    fn le_detail_resume() {
        assert_eq!(detail_resume(r#"{"de":"ENVOYEE","vers":"PAYEE"}"#), "Envoyée \u{2192} Payée");
        assert_eq!(
            detail_resume(r#"{"old_statut":"BROUILLON","new_statut":"ENVOYEE","journal_entry_id":88}"#),
            "Brouillon \u{2192} Envoyée"
        );
        // Un paiement : le montant, le mode, la référence, le solde, la date (les ids de l'écriture ne se disent pas).
        assert_eq!(
            detail_resume(concat!(
                r#"{"montant":1149.75,"mode_paiement":"cheque","reference":"1043","new_solde":"0.00","#,
                r#""new_statut":"PAYEE","date_paiement":"2026-10-28","je_id":5,"type_destinataire":"client"}"#
            )),
            "1\u{202f}149,75\u{a0}$ · solde 0,00\u{a0}$ · chèque · réf. 1043 · payée le 28 oct. 2026 · statut : Payée"
        );
        // Un envoi : le changement de statut, puis l'adresse.
        assert_eq!(
            detail_resume(concat!(
                r#"{"to_email":"x@exemple.ca","cc":null,"previous_statut":"BROUILLON","new_statut":"ENVOYEE","#,
                r#""pdf_size_bytes":53211}"#
            )),
            "Brouillon \u{2192} Envoyée · à x@exemple.ca"
        );
        // Un compte modifié : ses champs, avant et après.
        assert_eq!(
            detail_resume(r#"{"nom_avant":"Ventes","nom_apres":"Ventes de services","code":"4100"}"#),
            "nom : Ventes \u{2192} Ventes de services · 4100"
        );
        assert_eq!(detail_resume(r#"{"inserees":12,"doublons":1,"rejetees":0}"#), "12 ajoutées · 1 doublon");
        // Un compte du plan comptable modifié (`update_account`) : ses champs sensibles, avant et après, accentués.
        assert_eq!(
            detail_resume(r#"{"code":"4100","fields":["type"],"type_avant":"REVENU","type_apres":"ACTIF"}"#),
            "type : Revenu \u{2192} Actif · 4100 · champs : type"
        );
        assert_eq!(detail_resume(r#"{"rappels_actifs":false}"#), "rappels désactivés");
        // Une clé inconnue : telle quelle, après les connues.
        assert_eq!(
            detail_resume(r#"{"montant":"10","couleur_du_ciel":"bleu"}"#),
            "10,00\u{a0}$ · couleur du ciel : bleu"
        );
        // Un détail coupé au milieu par le serveur (300 caractères) : ses paires telles qu'elles se lisent encore.
        assert_eq!(detail_resume(r#"{"raison":"Erreur de quantit"#), "raison : Erreur de quantit…");
        assert_eq!(
            detail_resume(r#"{"montant":1149.75,"mode_paiement":"che"#),
            "montant : 1149.75 · mode_paiement : che…"
        );
        assert_eq!(detail_resume(""), "");
        assert_eq!(detail_resume("{}"), "");
    }

    fn registre() -> Registre {
        let facture = |id: i64, numero: &str, cote: Cote, tiers: &str, total: i64| Facture {
            id,
            numero: numero.into(),
            cote,
            tiers: tiers.into(),
            type_document: "FACTURE".into(),
            statut: "ENVOYEE".into(),
            total: Some(total),
            ..Facture::default()
        };
        let changement =
            |id: i64, quand: &str, qui: &str, action: &str, entite: &str, eid: Option<i64>, d: &str| Changement {
                id,
                quand: quand.into(),
                qui: qui.into(),
                action: action.into(),
                entite: entite.into(),
                entite_id: eid,
                details: d.into(),
            };
        Registre {
            factures: vec![
                facture(104, "FF-104", Cote::Fournisseur, "Béton Fictif ltée", 114_975),
                facture(105, "FACT-2026-00061", Cote::Client, "Groupe Fictif inc.", 2_299_500),
            ],
            historique: vec![
                changement(
                    9,
                    "2026-11-02T16:04:00Z",
                    "marie@exemple.ca",
                    "pay",
                    "invoice",
                    Some(105),
                    r#"{"montant":"23000","mode_paiement":"virement"}"#,
                ),
                changement(
                    8,
                    "2026-11-01T21:30:00Z",
                    "",
                    "send_reminder",
                    "invoice",
                    Some(104),
                    r#"{"niveau":2,"auto":true}"#,
                ),
                changement(
                    7,
                    "2026-10-28T13:00:00Z",
                    "mcp:tenant_x",
                    "journal_entry_create",
                    "journal_entry",
                    Some(12),
                    r#"{"numero":"JE-2026-00012","total":350.5}"#,
                ),
                changement(
                    6,
                    "2026-10-27T13:00:00Z",
                    "luc@exemple.ca",
                    "delete",
                    "invoice",
                    Some(999),
                    r#"{"statut":"BROUILLON"}"#,
                ),
                changement(5, "pas une date", "luc@exemple.ca", "archive_everything", "widget_x", None, ""),
            ],
            ..Registre::default()
        }
    }

    #[test]
    fn les_entrees_de_l_historique() {
        let r = registre();
        let e = entrees(&r, "2026-11-02", &montreal);
        assert_eq!(e.len(), 5);
        assert_eq!(
            (e[0].quand.as_str(), e[0].qui.as_str(), e[0].quoi.as_str()),
            ("aujourd'hui à 11 h 04", "marie@exemple.ca", "Paiement reçu")
        );
        assert_eq!((e[0].sur.as_str(), e[0].facture), ("FACT-2026-00061 · Groupe Fictif inc.", Some(1)));
        assert_eq!(e[0].detail, "23\u{202f}000,00\u{a0}$ · virement");
        assert_eq!(
            (e[1].quand.as_str(), e[1].qui.as_str(), e[1].sur.as_str()),
            ("hier à 16 h 30", "Système", "FF-104 · Béton Fictif ltée")
        );
        assert_eq!((e[1].quoi.as_str(), e[1].detail.as_str()), ("Rappel de paiement envoyé", "rappel 2 · automatique"));
        // Une écriture : ce qu'elle est, et son id ; un agent IA par le serveur MCP.
        assert_eq!(
            (e[2].qui.as_str(), e[2].sur.as_str(), e[2].facture),
            ("Agent IA (MCP)", "Écriture de journal (id 12)", None)
        );
        assert_eq!(e[2].detail, "JE-2026-00012 · total 350,50\u{a0}$");
        // Une facture que la lecture n'a plus (supprimée) : nommée par son id.
        assert_eq!(
            (e[3].quoi.as_str(), e[3].sur.as_str(), e[3].facture),
            ("Facture supprimée", "Facture (id 999)", None)
        );
        // L'inconnu : tel quel, l'instant illisible aussi.
        assert_eq!(
            (e[4].quand.as_str(), e[4].quoi.as_str(), e[4].sur.as_str()),
            ("pas une date", "archive_everything", "widget_x")
        );
    }

    #[test]
    fn la_recherche_en_texte() {
        let e = entrees(&registre(), "2026-11-02", &montreal);
        assert_eq!(retenues(&e, ""), [0, 1, 2, 3, 4]);
        assert_eq!(retenues(&e, "béton"), [1], "sans la casse ni les accents : « Béton » et « beton »");
        assert_eq!(retenues(&e, "BETON fictif"), [1]);
        assert_eq!(retenues(&e, "fictif"), [0, 1]);
        assert_eq!(retenues(&e, "marie virement"), [0], "chaque mot");
        assert_eq!(retenues(&e, "marie rappel"), Vec::<usize>::new());
        assert_eq!(retenues(&e, "28 oct"), [2]);
        assert_eq!(retenues(&e, "systeme"), [1]);
    }

    #[test]
    fn ce_qu_on_demande_a_eve() {
        let r = registre();
        let e = entrees(&r, "2026-11-02", &montreal);
        assert_eq!(
            demande(&e[0], Some(&r.factures[1])),
            "Parlons de la facture FACT-2026-00061 à Groupe Fictif inc., 22\u{202f}995,00\u{a0}$ (id 105 dans \
             Constructo AI), vue à l'historique de la comptabilité. Au journal : Paiement reçu aujourd'hui à 11 h 04 \
             par marie@exemple.ca \u{2014} 23\u{202f}000,00\u{a0}$ · virement."
        );
        assert_eq!(
            demande(&e[2], None),
            "Parlons de ce changement, vu à l'historique de la comptabilité : Écriture créée le 28 oct. à 9 h par un \
             agent IA (MCP), sur : Écriture de journal (id 12) \u{2014} JE-2026-00012 · total 350,50\u{a0}$."
        );
        // Le rappel se lira demain : le jour en entier, jamais « hier ».
        assert_eq!(
            rappel(&e[1], Some(&r.factures[0])),
            "Comptabilité : FF-104 · Béton Fictif ltée, 1\u{202f}149,75\u{a0}$ (rappel de paiement envoyé le 1er nov. \
             2026 à 16 h 30)"
        );
        assert_eq!(rappel(&e[4], None), "Comptabilité : archive_everything, widget_x (pas une date)");
    }
}
