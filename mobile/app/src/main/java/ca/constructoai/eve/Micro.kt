package ca.constructoai.eve

import android.annotation.SuppressLint
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import java.io.ByteArrayOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.concurrent.thread
import kotlin.math.sqrt

/**
 * Le micro du téléphone, tenu pour parler : du son à 16 kHz, mono, 16 bits — ce que l'oreille de l'ordinateur
 * (whisper) attend, sans rien recalculer —, 45 secondes au plus (comme la touche de l'ordinateur). La source de la
 * reconnaissance vocale : ni gain automatique ni réduction de bruit, que l'oreille fait mieux.
 */
class Micro {
    private var enregistreur: AudioRecord? = null
    private val son = ByteArrayOutputStream()

    @Volatile
    private var ecoute = false

    @Volatile
    private var niveauCourant = 0f

    /** Le niveau de la voix, de 0 à 1 (l'onde du bouton). */
    val niveau: Float get() = niveauCourant

    /** Commence d'écouter ; faux si le micro ne s'ouvre pas (la permission, un autre programme qui le tient). */
    @SuppressLint("MissingPermission")
    fun commencer(): Boolean {
        if (ecoute) return true
        val tampon = AudioRecord.getMinBufferSize(CADENCE, AudioFormat.CHANNEL_IN_MONO, AudioFormat.ENCODING_PCM_16BIT)
        val r = runCatching {
            AudioRecord(MediaRecorder.AudioSource.VOICE_RECOGNITION, CADENCE, AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT, maxOf(tampon, CADENCE))
        }.getOrNull()
        if (r == null || r.state != AudioRecord.STATE_INITIALIZED) {
            r?.release()
            return false
        }
        son.reset()
        enregistreur = r
        ecoute = true
        r.startRecording()
        thread(name = "micro-d-eve", isDaemon = true) {
            val morceau = ByteArray(3200)
            while (ecoute && son.size() < MAX) {
                val n = r.read(morceau, 0, morceau.size)
                if (n <= 0) break
                synchronized(son) { son.write(morceau, 0, n) }
                niveauCourant = (rms(morceau, n) * 6f).coerceAtMost(1f)
            }
        }
        return true
    }

    /** Le son tenu, en WAV ; rien s'il est trop court pour être une phrase (un doigt qui glisse). */
    fun finir(): ByteArray? {
        if (!ecoute) return null
        ecoute = false
        niveauCourant = 0f
        enregistreur?.let { runCatching { it.stop() }; it.release() }
        enregistreur = null
        val pcm = synchronized(son) { son.toByteArray() }
        if (pcm.size < CADENCE * 2 * 35 / 100) return null
        return wav(pcm)
    }

    private fun rms(o: ByteArray, n: Int): Float {
        val b = ByteBuffer.wrap(o, 0, n).order(ByteOrder.LITTLE_ENDIAN)
        var somme = 0.0
        val k = n / 2
        for (i in 0 until k) {
            val v = b.getShort(i * 2) / 32768.0
            somme += v * v
        }
        return if (k == 0) 0f else sqrt(somme / k).toFloat()
    }

    companion object {
        const val CADENCE = 16_000

        /** 45 secondes de son. */
        private const val MAX = CADENCE * 2 * 45

        /** Un WAV PCM 16 bits mono à 16 kHz autour de ces échantillons. */
        fun wav(pcm: ByteArray): ByteArray {
            val b = ByteBuffer.allocate(44 + pcm.size).order(ByteOrder.LITTLE_ENDIAN)
            b.put("RIFF".toByteArray()).putInt(36 + pcm.size).put("WAVE".toByteArray())
            b.put("fmt ".toByteArray()).putInt(16).putShort(1).putShort(1).putInt(CADENCE).putInt(CADENCE * 2)
                .putShort(2).putShort(16)
            b.put("data".toByteArray()).putInt(pcm.size).put(pcm)
            return b.array()
        }
    }
}
