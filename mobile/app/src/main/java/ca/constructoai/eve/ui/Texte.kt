package ca.constructoai.eve.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withStyle

/** Un morceau d'une réponse d'Eve : du texte mis en forme, le détail d'après les trois tirets, un bloc de code, ou une
 * carte (un tableau, un dessin…), que l'appli ne dessine pas encore. */
sealed interface Morceau {
    data class Texte(val texte: AnnotatedString, val detail: Boolean) : Morceau
    data class Code(val texte: String) : Morceau
    data class Carte(val sorte: String) : Morceau
}

private val CARTES = mapOf(
    "tableau" to "un tableau", "agenda" to "un agenda", "brouillon" to "un brouillon", "apercu" to "un aperçu",
    "rappel" to "un rappel", "epingle" to "une carte du tableau de bord", "graphique" to "un graphique",
)

/**
 * Une réponse d'Eve en morceaux, comme la fenêtre de l'ordinateur la met en forme : le gras, l'italique, le code,
 * les puces et les titres ; après « --- », le détail, que la voix n'a pas dit ; les blocs de code ; et les cartes
 * (` ```carte `, ` ```svg `), dites par leur sorte.
 */
fun morceaux(texte: String, encreDuCode: Color): List<Morceau> {
    val sortie = mutableListOf<Morceau>()
    val lignes = texte.replace("\r\n", "\n").split('\n')
    var detail = false
    val paragraphe = mutableListOf<String>()
    fun finirLeParagraphe() {
        if (paragraphe.isNotEmpty()) {
            sortie += Morceau.Texte(enForme(paragraphe.joinToString("\n"), encreDuCode), detail)
            paragraphe.clear()
        }
    }
    var i = 0
    while (i < lignes.size) {
        val l = lignes[i]
        val t = l.trim()
        if (t.startsWith("```")) {
            finirLeParagraphe()
            val nom = t.removePrefix("```").trim().lowercase()
            val bloc = mutableListOf<String>()
            i++
            while (i < lignes.size && !lignes[i].trim().startsWith("```")) bloc += lignes[i++]
            val contenu = bloc.joinToString("\n")
            sortie += when {
                nom == "svg" || contenu.trimStart().startsWith("<svg") -> Morceau.Carte("un dessin")
                nom == "carte" || nom == "rappel" -> {
                    val sorte = Regex("\"type\"\\s*:\\s*\"([a-z_]+)\"").find(contenu)?.groupValues?.get(1)
                    Morceau.Carte(CARTES[sorte] ?: "une carte")
                }
                else -> Morceau.Code(contenu)
            }
        } else if (t == "---" || t == "***") {
            finirLeParagraphe()
            detail = true
        } else if (t.isEmpty()) {
            finirLeParagraphe()
        } else {
            paragraphe += l
        }
        i++
    }
    finirLeParagraphe()
    return sortie
}

/** Le markdown d'un paragraphe : `**gras**`, `*italique*`, `` `code` ``, les puces, les titres, `[un lien](…)`. */
fun enForme(texte: String, encreDuCode: Color): AnnotatedString = buildAnnotatedString {
    texte.split('\n').forEachIndexed { n, brute ->
        if (n > 0) append('\n')
        var l = brute
        val puce = Regex("^\\s*[-*•]\\s+").find(l)
        val numero = Regex("^\\s*(\\d+)[.)]\\s+").find(l)
        val titre = Regex("^#{1,6}\\s+").find(l)
        when {
            puce != null -> { append("•  "); l = l.substring(puce.range.last + 1) }
            numero != null -> { append("${numero.groupValues[1]}.  "); l = l.substring(numero.range.last + 1) }
        }
        if (titre != null) {
            withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { enLigne(l.substring(titre.range.last + 1), encreDuCode) }
        } else {
            enLigne(l, encreDuCode)
        }
    }
}

private fun AnnotatedString.Builder.enLigne(l: String, encreDuCode: Color) {
    var i = 0
    while (i < l.length) {
        when {
            l.startsWith("**", i) && l.indexOf("**", i + 2) > i + 2 -> {
                val fin = l.indexOf("**", i + 2)
                withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { enLigne(l.substring(i + 2, fin), encreDuCode) }
                i = fin + 2
            }
            l[i] == '`' && l.indexOf('`', i + 1) > i + 1 -> {
                val fin = l.indexOf('`', i + 1)
                withStyle(SpanStyle(fontFamily = FontFamily.Monospace, color = encreDuCode)) {
                    append(l.substring(i + 1, fin))
                }
                i = fin + 1
            }
            l[i] == '*' && i + 1 < l.length && l[i + 1] != ' ' && l.indexOf('*', i + 1) > i + 1 -> {
                val fin = l.indexOf('*', i + 1)
                withStyle(SpanStyle(fontStyle = androidx.compose.ui.text.font.FontStyle.Italic)) {
                    append(l.substring(i + 1, fin))
                }
                i = fin + 1
            }
            l[i] == '[' && Regex("^\\[([^]]+)]\\(([^)]+)\\)").find(l.substring(i)) != null -> {
                val m = Regex("^\\[([^]]+)]\\(([^)]+)\\)").find(l.substring(i))!!
                withStyle(SpanStyle(textDecoration = TextDecoration.Underline)) { append(m.groupValues[1]) }
                i += m.value.length
            }
            else -> append(l[i++])
        }
    }
}
