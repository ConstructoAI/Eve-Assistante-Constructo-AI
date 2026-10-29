package ca.constructoai.eve

import org.json.JSONArray
import org.json.JSONObject

/** Une ligne de la conversation : de qui (« toi », « eve », « systeme », « alerte »…), son texte, son heure, ses pièces. */
data class Ligne(val de: String, val texte: String, val heure: String, val pieces: List<String>, val tour: String)

/** Une écriture ou une suppression qui attend l'accord de la personne : son numéro (le geste le redonne), ce qu'elle est. */
data class Accord(val numero: Long, val quoi: String, val detail: String, val restants: Int)

/** Une pièce jointe à la prochaine demande, déjà sur l'ordinateur. */
data class PieceJointe(val id: String, val nom: String, val taille: Long)

/**
 * L'état de l'Eve de l'ordinateur, tel que son flux le donne au téléphone (`src\telephone.rs` : `alleger`) : ce qu'elle
 * fait (`etat` : pret, ecoute, transcription, reflexion, parole, confirmation, demarrage, erreur), la conversation, la
 * réponse qui s'écrit, l'accord qu'elle attend, les pièces jointes.
 */
data class EtatEve(
    val etat: String = "demarrage",
    val libelle: String = "",
    val aide: String = "",
    val emotion: String? = null,
    val nom: String = "",
    val qui: String = "",
    val profil: String = "",
    val accord: Accord? = null,
    val fil: List<Ligne> = emptyList(),
    val filRev: Long = -1,
    val enCours: String? = null,
    val pieces: List<PieceJointe> = emptyList(),
    val version: String = "",
) {
    /** L'état que porte cet événement du flux ; sans `fil` (la conversation n'a pas changé), celle d'avant reste. */
    fun suivant(json: String): EtatEve {
        val o = runCatching { JSONObject(json) }.getOrNull() ?: return this
        val fil = o.optJSONArray("fil")?.let(::lignes) ?: this.fil
        return EtatEve(
            etat = o.optString("etat", etat),
            libelle = o.optString("libelle"),
            aide = o.optString("aide"),
            emotion = o.optString("emotion").takeIf { it.isNotEmpty() && !o.isNull("emotion") },
            nom = o.optString("nom", nom),
            qui = o.optString("qui", qui),
            profil = o.optString("profil", profil),
            accord = o.optJSONObject("accord")?.let {
                Accord(it.optLong("numero"), it.optString("quoi"), it.optString("detail"), it.optInt("restants"))
            },
            fil = fil,
            filRev = o.optLong("fil_rev", filRev),
            enCours = o.optJSONObject("en_cours")?.optString("texte")?.takeIf(String::isNotBlank),
            pieces = o.optJSONArray("pieces")?.let { a ->
                (0 until a.length()).map { a.getJSONObject(it) }
                    .map { PieceJointe(it.optString("id"), it.optString("nom"), it.optLong("taille")) }
            } ?: emptyList(),
            version = o.optJSONObject("version")?.optString("texte") ?: version,
        )
    }

    private fun lignes(a: JSONArray): List<Ligne> = (0 until a.length()).map { a.getJSONObject(it) }.map { l ->
        val pieces = l.optJSONArray("pieces")
        Ligne(
            de = l.optString("de"),
            texte = l.optString("texte"),
            heure = l.optString("heure"),
            pieces = if (pieces == null) emptyList() else (0 until pieces.length()).map { pieces.getString(it) },
            tour = l.optString("tour"),
        )
    }

    /** Eve travaille, écrit ou parle : un geste « arrêter » a un sens. */
    val occupee: Boolean get() = etat in setOf("transcription", "reflexion", "parole", "confirmation")
}
