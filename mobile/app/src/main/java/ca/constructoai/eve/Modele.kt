package ca.constructoai.eve

import android.app.Application
import android.graphics.BitmapFactory
import android.os.Build
import android.util.Log
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import okhttp3.Response
import okhttp3.sse.EventSource
import okhttp3.sse.EventSourceListener
import org.json.JSONObject
import java.util.Base64

/** La liaison avec l'ordinateur, telle que la barre de l'appli la montre. */
enum class EtatDeLaLiaison { Hors, EnCours, Branchee, Refusee }

/**
 * Ce que tient l'appli : son jumelage, la liaison avec l'Eve de l'ordinateur (son flux, rouvert après une panne, d'une
 * adresse à l'autre), l'état qu'elle publie, sa voix et le micro. Les écrans ne font que le montrer et lui rendre des
 * gestes.
 */
class Modele(application: Application) : AndroidViewModel(application) {
    private val coffre = Coffre(application)

    var jumelage by mutableStateOf(coffre.lire())
        private set
    var eve by mutableStateOf(EtatEve())
        private set
    var liaison by mutableStateOf(EtatDeLaLiaison.Hors)
        private set

    /** Ce que l'appli a à dire de la liaison ou d'un geste qui n'est pas passé ; rien : tout va. */
    var message by mutableStateOf<String?>(null)
        private set
    var portrait by mutableStateOf<ImageBitmap?>(null)
        private set
    var ecoute by mutableStateOf(false)
        private set
    var jumelageEnCours by mutableStateOf(false)
        private set

    /** La voix d'Eve sur ce téléphone, et la sourdine du téléphone : la réponse à une demande d'ici ne se dit qu'ici, et
     * la sourdine la tait (le texte reste). */
    val voix = Voix()
    var sourdine by mutableStateOf(false)
        private set
    private val micro = Micro()
    val niveauDuMicro: Float get() = micro.niveau

    private var lien: Liaison? = null
    private var source: EventSource? = null
    private var pannes = 0

    init {
        jumelage?.let(::brancher)
    }

    /** Un code carré lu, ou un lien `eve://jumeler…` ouvert : le jumelage avec cette Eve. */
    fun jumeler(texte: String) {
        val offre = Offre.duLien(texte)
        if (offre == null) {
            message = "Ce code n'est pas celui d'une Eve : sur l'ordinateur, Réglages › Téléphones › Jumeler un téléphone."
            return
        }
        jumelageEnCours = true
        message = null
        viewModelScope.launch {
            jumeler(offre, "${Build.MANUFACTURER.replaceFirstChar(Char::uppercase)} ${Build.MODEL}".trim())
                .onSuccess { j ->
                    coffre.ecrire(j)
                    jumelage = j
                    brancher(j)
                }
                .onFailure { message = it.message }
            jumelageEnCours = false
        }
    }

    /** Oublie le jumelage : le téléphone n'a plus de clé ; sur l'ordinateur, il reste à le retirer des réglages. */
    fun oublier() {
        debrancher()
        coffre.oublier()
        jumelage = null
        eve = EtatEve()
        portrait = null
        message = null
    }

    fun ecrire(texte: String) {
        if (texte.isNotBlank()) geste("texte", texte.trim())
    }

    fun confirmer(a: Accord) = geste("confirmer", a.numero)
    fun annuler(a: Accord) = geste("annuler", a.numero)
    fun nouvelle() = geste("nouvelle")
    fun retirerPiece(p: PieceJointe) = geste("piece_retirer", p.id)

    /** Arrête Eve : ce qu'elle dit, et ce qu'elle fait. */
    fun arreter() {
        voix.couper()
        geste("arreter")
    }

    fun basculerLaSourdine() {
        sourdine = !sourdine
        voix.volume = if (sourdine) 0f else 1f
        if (sourdine) voix.couper()
    }

    /** Le bouton du micro, tenu : Eve se tait si elle parlait (son travail continue), puis le micro écoute. */
    fun parlerDebut(): Boolean {
        if (voix.parle.value) {
            voix.couper()
            geste("taire")
        }
        ecoute = micro.commencer()
        if (!ecoute) message = "Le micro ne s'ouvre pas : autorise-le à Eve dans les réglages du téléphone."
        return ecoute
    }

    /** Le bouton relâché : ce qui a été dit part à l'ordinateur, que son oreille transcrit. */
    fun parlerFin() {
        val wav = micro.finir()
        ecoute = false
        val l = lien ?: return
        if (wav == null) return
        viewModelScope.launch {
            val code = l.son(wav)
            if (code != 204) message = "Ta voix n'est pas partie : ${raison(code)}"
        }
    }

    /** Une photo, ou un fichier, jointe à la prochaine demande. */
    fun joindre(nom: String, octets: ByteArray) {
        val l = lien ?: return
        viewModelScope.launch {
            val (code, texte) = l.piece(nom, octets)
            if (code != 200) message = texte.ifBlank { "« $nom » n'est pas parti : ${raison(code)}" }
        }
    }

    fun oublierLeMessage() {
        message = null
    }

    private fun geste(action: String, valeur: Any? = null) {
        val l = lien ?: return
        viewModelScope.launch {
            val code = l.geste(action, valeur)
            if (code != 204) message = "L'ordinateur n'a pas reçu ce geste : ${raison(code)}"
        }
    }

    private fun raison(code: Int) = when (code) {
        0 -> "il ne répond pas."
        401 -> "cette Eve ne connaît plus ce téléphone."
        else -> "il a répondu $code."
    }

    private fun brancher(j: Jumelage) {
        val l = Liaison(j)
        lien = l
        liaison = EtatDeLaLiaison.EnCours
        viewModelScope.launch {
            l.portrait()?.let { o -> BitmapFactory.decodeByteArray(o, 0, o.size)?.asImageBitmap() }?.let { portrait = it }
        }
        ouvrirLeFlux(l)
    }

    private fun debrancher() {
        source?.cancel()
        source = null
        lien = null
        voix.couper()
        liaison = EtatDeLaLiaison.Hors
    }

    private fun ouvrirLeFlux(l: Liaison) {
        source = l.flux(object : EventSourceListener() {
            override fun onOpen(eventSource: EventSource, response: Response) {
                viewModelScope.launch {
                    if (lien !== l) return@launch
                    pannes = 0
                    liaison = EtatDeLaLiaison.Branchee
                    message = null
                }
            }

            override fun onEvent(eventSource: EventSource, id: String?, type: String?, data: String) {
                when (type) {
                    // La voix se joue dès qu'elle arrive, hors du fil de l'écran.
                    "voix" -> runCatching {
                        val o = JSONObject(data)
                        Log.i("Eve", "voix : la phrase ${o.optLong("numero")} reçue")
                        voix.jouer(o.optLong("pause"), Base64.getUrlDecoder().decode(o.getString("wav")))
                    }
                    "coupe" -> voix.couper()
                    "etat" -> viewModelScope.launch { if (lien === l) eve = eve.suivant(data) }
                }
            }

            override fun onClosed(eventSource: EventSource) = reessayer(l, null, null)

            override fun onFailure(eventSource: EventSource, t: Throwable?, response: Response?) =
                reessayer(l, t, response?.code)
        })
    }

    /** Le flux s'est fermé : l'ordinateur ne connaît plus ce téléphone (401), ou la liaison a flanché — il se rouvre,
     * par une autre adresse, de plus en plus lentement (une, deux, quatre… quinze secondes au plus). */
    private fun reessayer(l: Liaison, t: Throwable?, code: Int?) {
        viewModelScope.launch {
            if (lien !== l) return@launch
            if (code == 401) {
                liaison = EtatDeLaLiaison.Refusee
                message = "Cette Eve ne connaît plus ce téléphone : rejumelle-le (Réglages › Téléphones, sur l'ordinateur)."
                return@launch
            }
            liaison = EtatDeLaLiaison.EnCours
            if (pannes >= 1) message = t?.let(::explication) ?: "La liaison avec l'ordinateur s'est coupée."
            pannes++
            l.autreAdresse()
            delay(1000L * minOf(15, 1 shl minOf(pannes - 1, 4)))
            if (lien === l) ouvrirLeFlux(l)
        }
    }

    override fun onCleared() {
        debrancher()
        micro.finir()
    }
}
