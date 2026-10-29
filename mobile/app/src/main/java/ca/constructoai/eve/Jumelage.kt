package ca.constructoai.eve

import org.json.JSONArray
import org.json.JSONObject
import java.net.URLDecoder

/**
 * L'offre de jumelage que porte le code carré de l'ordinateur (`src\telephone.rs` : `lien`) :
 * `eve://jumeler?v=1&n=<nom de l'Eve>&a=<adresses>&p=<port>&e=<empreinte>&c=<code>`. Le code ne sert qu'une fois, dans
 * les cinq minutes ; l'empreinte est le SHA-256 du certificat de l'ordinateur, en base64url : le seul que l'appli
 * acceptera.
 */
data class Offre(
    val nom: String,
    val adresses: List<String>,
    val port: Int,
    val empreinte: String,
    val code: String,
) {
    companion object {
        /** L'offre d'un lien, ou rien s'il n'en est pas une (un autre code carré, un lien abîmé). */
        fun duLien(lien: String): Offre? {
            val l = lien.trim()
            if (!l.startsWith("eve://jumeler?")) return null
            val parametres = l.substringAfter('?').split('&').mapNotNull { p ->
                val i = p.indexOf('=')
                if (i <= 0) null else p.substring(0, i) to URLDecoder.decode(p.substring(i + 1), "UTF-8")
            }.toMap()
            if (parametres["v"] != "1") return null
            val adresses = parametres["a"].orEmpty().split(',').map(String::trim).filter(String::isNotEmpty)
            val port = parametres["p"]?.toIntOrNull()?.takeIf { it in 1..65535 } ?: return null
            val empreinte = parametres["e"]?.takeIf { it.length == 43 } ?: return null
            val code = parametres["c"]?.takeIf { it.length == 43 } ?: return null
            if (adresses.isEmpty()) return null
            return Offre(parametres["n"].orEmpty().ifBlank { "Eve" }, adresses, port, empreinte, code)
        }
    }
}

/**
 * Ce que le téléphone garde de son jumelage : où joindre l'Eve de l'ordinateur (ses adresses, son port), comment la
 * reconnaître (l'empreinte de son certificat), son nom, la personne pour qui il s'est jumelé, et sa clé à lui — que
 * l'ordinateur ne connaît que par son SHA-256. Chiffré dans le téléphone ([Coffre]).
 */
data class Jumelage(
    val adresses: List<String>,
    val port: Int,
    val empreinte: String,
    val eve: String,
    val id: String,
    val qui: String,
    val cle: String,
) {
    fun enJson(): String = JSONObject()
        .put("adresses", JSONArray(adresses))
        .put("port", port)
        .put("empreinte", empreinte)
        .put("eve", eve)
        .put("id", id)
        .put("qui", qui)
        .put("cle", cle)
        .toString()

    companion object {
        fun deJson(s: String): Jumelage? = runCatching {
            val o = JSONObject(s)
            val a = o.getJSONArray("adresses")
            Jumelage(
                adresses = (0 until a.length()).map { a.getString(it) },
                port = o.getInt("port"),
                empreinte = o.getString("empreinte"),
                eve = o.getString("eve"),
                id = o.getString("id"),
                qui = o.optString("qui"),
                cle = o.getString("cle"),
            )
        }.getOrNull()
    }
}
