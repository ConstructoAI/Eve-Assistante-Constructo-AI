package ca.constructoai.eve

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.util.Log
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.LinkedBlockingQueue
import kotlin.concurrent.thread

/** Un WAV PCM 16 bits : sa cadence, ses canaux, ses échantillons. */
class Pcm(val cadence: Int, val canaux: Int, val octets: ByteArray, val debut: Int, val taille: Int) {
    val duree: Long get() = taille * 1000L / (cadence * canaux * 2).coerceAtLeast(1)

    companion object {
        /** Le PCM d'un WAV (son bloc `fmt `, puis `data`) ; rien si ce n'en est pas un. */
        fun du(wav: ByteArray): Pcm? {
            if (wav.size < 12 || String(wav, 0, 4) != "RIFF" || String(wav, 8, 4) != "WAVE") return null
            val b = ByteBuffer.wrap(wav).order(ByteOrder.LITTLE_ENDIAN)
            var i = 12
            var (cadence, canaux, bits) = Triple(22050, 1, 16)
            while (i + 8 <= wav.size) {
                val id = String(wav, i, 4)
                val taille = b.getInt(i + 4)
                val d = i + 8
                if (id == "fmt " && d + 16 <= wav.size) {
                    canaux = b.getShort(d + 2).toInt()
                    cadence = b.getInt(d + 4)
                    bits = b.getShort(d + 14).toInt()
                } else if (id == "data") {
                    if (bits != 16) return null
                    return Pcm(cadence, canaux.coerceIn(1, 2), wav, d, minOf(taille, wav.size - d) and 1.inv())
                }
                i = d + taille + (taille and 1)
            }
            return null
        }
    }
}

/**
 * La voix d'Eve sur le téléphone : ses phrases arrivent du flux (un WAV chacune, Jessica faite sur l'ordinateur), se
 * jouent dans l'ordre, chacune après le silence que l'ordinateur a compté (la ponctuation, un autre paragraphe) — mais
 * jamais avant la fin de la précédente. `couper` tait ce qui se dit et oublie ce qui attend.
 */
class Voix {
    private class Phrase(val generation: Int, val pause: Long, val pcm: Pcm)

    private val file = LinkedBlockingQueue<Phrase>()

    @Volatile
    private var generation = 0

    @Volatile
    private var piste: AudioTrack? = null

    private val parleFlux = MutableStateFlow(false)

    /** Eve parle sur le téléphone. */
    val parle: StateFlow<Boolean> = parleFlux

    /** Le volume de sa voix : 0 la tait (la sourdine du téléphone), 1 tel quel. */
    @Volatile
    var volume = 1f

    init {
        thread(name = "voix-d-eve", isDaemon = true) { mener() }
    }

    fun jouer(pause: Long, wav: ByteArray) {
        val pcm = Pcm.du(wav) ?: return
        file.put(Phrase(generation, pause, pcm))
    }

    fun couper() {
        generation++
        file.clear()
        piste?.let { runCatching { it.pause(); it.flush() } }
    }

    private fun mener() {
        var finDeLaPrecedente = 0L
        while (true) {
            val p = file.take()
            if (p.generation != generation) continue
            parleFlux.value = true
            val attente = finDeLaPrecedente + p.pause - System.currentTimeMillis()
            if (attente > 0) Thread.sleep(attente.coerceAtMost(3000))
            if (p.generation != generation) continue
            // Un son qui ne se joue pas (le son du téléphone pris, une sortie qui manque) se perd seul : la voix
            // continue avec le suivant.
            runCatching { jouerLaPhrase(p) }.onFailure { Log.w("Eve", "la voix ne se joue pas : $it") }
            finDeLaPrecedente = System.currentTimeMillis()
            if (file.isEmpty()) parleFlux.value = false
        }
    }

    private fun jouerLaPhrase(p: Phrase) {
        val piste = pisteDe(p.pcm.cadence, p.pcm.canaux)
        piste.setVolume(volume)
        piste.play()
        var ecrit = 0
        while (ecrit < p.pcm.taille && p.generation == generation) {
            val n = piste.write(p.pcm.octets, p.pcm.debut + ecrit, minOf(8192, p.pcm.taille - ecrit))
            if (n <= 0) break
            ecrit += n
        }
        // Le son écrit se joue encore : la fin de sa voix, d'après la tête de lecture (au plus sa durée et une seconde,
        // si la tête ne bouge pas).
        val images = ecrit / (2 * p.pcm.canaux)
        val limite = System.currentTimeMillis() + p.pcm.duree + 1000
        while (p.generation == generation && piste.playbackHeadPosition < images && System.currentTimeMillis() < limite) {
            Thread.sleep(20)
        }
        piste.stop()
        Log.i("Eve", "voix : une phrase de ${p.pcm.duree} ms jouée")
    }

    /** Une piste à la cadence de ce son ; celle d'avant, si c'est la même. */
    private fun pisteDe(cadence: Int, canaux: Int): AudioTrack {
        piste?.let { if (it.sampleRate == cadence && it.channelCount == canaux) return it.also { p -> p.flush() } }
        piste?.release()
        val masque = if (canaux == 2) AudioFormat.CHANNEL_OUT_STEREO else AudioFormat.CHANNEL_OUT_MONO
        val tampon = AudioTrack.getMinBufferSize(cadence, masque, AudioFormat.ENCODING_PCM_16BIT)
        val nouvelle = AudioTrack.Builder()
            .setAudioAttributes(
                AudioAttributes.Builder()
                    .setUsage(AudioAttributes.USAGE_ASSISTANT)
                    .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                    .build()
            )
            .setAudioFormat(
                AudioFormat.Builder().setSampleRate(cadence).setChannelMask(masque)
                    .setEncoding(AudioFormat.ENCODING_PCM_16BIT).build()
            )
            .setBufferSizeInBytes(maxOf(tampon, cadence))
            .setTransferMode(AudioTrack.MODE_STREAM)
            .build()
        piste = nouvelle
        return nouvelle
    }
}
