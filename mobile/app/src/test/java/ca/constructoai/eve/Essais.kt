package ca.constructoai.eve

import androidx.compose.ui.graphics.Color
import ca.constructoai.eve.ui.Morceau
import ca.constructoai.eve.ui.morceaux
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Les essais de l'appli qui se passent d'un téléphone (Construire.bat --essais). */
class Essais {
    private val empreinte = "E".repeat(43)
    private val code = "C".repeat(43)

    /** Le lien du code carré, tel que l'écrit l'ordinateur (`src\telephone.rs` : `lien`, et son essai). */
    @Test
    fun unLienDeJumelageSeLit() {
        val o = Offre.duLien(
            "eve://jumeler?v=1&n=Essai%20d%27%C3%89t%C3%A9&a=192.168.1.20,100.64.0.7&p=48822&e=$empreinte&c=$code"
        )
        assertNotNull(o)
        assertEquals("Essai d'Été", o!!.nom)
        assertEquals(listOf("192.168.1.20", "100.64.0.7"), o.adresses)
        assertEquals(48822, o.port)
        assertEquals(empreinte, o.empreinte)
        assertEquals(code, o.code)
    }

    @Test
    fun unAutreCodeNEstPasUneOffre() {
        assertNull(Offre.duLien("https://exemple.com/?a=1"))
        assertNull(Offre.duLien("eve://jumeler?v=2&n=E&a=10.0.0.1&p=1&e=$empreinte&c=$code"), )
        assertNull(Offre.duLien("eve://jumeler?v=1&n=E&a=&p=48822&e=$empreinte&c=$code"))
        assertNull(Offre.duLien("eve://jumeler?v=1&n=E&a=10.0.0.1&p=99999&e=$empreinte&c=$code"))
        assertNull(Offre.duLien("eve://jumeler?v=1&n=E&a=10.0.0.1&p=48822&e=court&c=$code"))
    }

    @Test
    fun unJumelageSeGardeEtSeRelit() {
        val j = Jumelage(listOf("192.168.1.20", "100.64.0.7"), 48822, empreinte, "Essai", "0123456789abcdef", "Sam", code)
        assertEquals(j, Jumelage.deJson(j.enJson()))
        assertNull(Jumelage.deJson("pas du JSON"))
    }

    /** Le WAV de la voix d'Eve : sa cadence, sa durée ; ce qui n'en est pas un ne se joue pas. */
    @Test
    fun unWavSeLit() {
        val wav = Micro.wav(ByteArray(32_000))
        val pcm = Pcm.du(wav)
        assertNotNull(pcm)
        assertEquals(16_000, pcm!!.cadence)
        assertEquals(1, pcm.canaux)
        assertEquals(32_000, pcm.taille)
        assertEquals(1000L, pcm.duree)
        assertNull(Pcm.du("pas un son".toByteArray()))
    }

    /** Une réponse d'Eve : le texte, le détail d'après les trois tirets, une carte dite par sa sorte, un bloc de code. */
    @Test
    fun uneReponseSeMetEnForme() {
        val texte = "Voici **le point**.\n\n- un\n- deux\n---\nLe détail.\n```carte\n{\"type\":\"tableau\"}\n```\n" +
            "```svg\n<svg></svg>\n```\n```\nexemple\n```"
        val m = morceaux(texte, Color.Blue)
        assertEquals(6, m.size)
        assertEquals("Voici le point.", (m[0] as Morceau.Texte).texte.text)
        assertEquals("•  un\n•  deux", (m[1] as Morceau.Texte).texte.text)
        assertTrue((m[2] as Morceau.Texte).detail)
        assertEquals(Morceau.Carte("un tableau"), m[3])
        assertEquals(Morceau.Carte("un dessin"), m[4])
        assertEquals(Morceau.Code("exemple"), m[5])
    }
}
