package fr.eveetlucas.homewidget

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import org.json.JSONObject
import java.io.File

/** Une image du widget photo, préparée par l'appli (`photos.rs`). */
data class PhotoImage(
  val id: String,
  val name: String,
  /** `true` : l'image remplit le widget (bords rognés) ; `false` : image entière. */
  val fill: Boolean,
  /** Intervalle entre deux images d'un GIF ; 0 pour une image fixe. */
  val frameMs: Int,
  /** Chemins des images différentes (PNG). */
  val frames: List<String>,
  /** Ordre d'affichage : indices dans [frames]. */
  val sequence: List<Int>
)

/** Ce que le widget photo affiche. */
data class PhotoContent(val images: List<PhotoImage>, val pending: Int) {
  companion object {
    /**
     * Lit les données envoyées par l'appli. Une image illisible est ignorée,
     * de même qu'un chemin hors du dossier privé de l'appli.
     */
    fun parse(context: Context, raw: String?): PhotoContent? {
      if (raw == null) return null
      return try {
        val json = JSONObject(raw)
        val dataDir = File(context.applicationInfo.dataDir).canonicalPath + File.separator
        val list = json.optJSONArray("images")
        val images = mutableListOf<PhotoImage>()
        if (list != null) {
          for (i in 0 until list.length()) {
            val image = readImage(list.optJSONObject(i), dataDir)
            if (image != null) images.add(image)
          }
        }
        PhotoContent(images, json.optInt("pending", 0))
      } catch (e: Exception) {
        Log.w(TAG, "Données du widget photo illisibles", e)
        null
      }
    }

    private fun readImage(json: JSONObject?, dataDir: String): PhotoImage? {
      if (json == null) return null
      val id = json.optStringOrNull("id") ?: return null
      val name = json.optStringOrNull("name") ?: ""
      val fit = json.optStringOrNull("fit") ?: return null
      val frameMs = (json.opt("frameMs") as? Number)?.toInt() ?: return null
      val rawFrames = json.optJSONArray("frames") ?: return null
      val frames = mutableListOf<String>()
      for (i in 0 until rawFrames.length()) {
        val path = rawFrames.opt(i) as? String ?: return null
        if (!File(path).canonicalPath.startsWith(dataDir)) return null
        frames.add(path)
      }
      val rawSequence = json.optJSONArray("sequence") ?: return null
      val sequence = mutableListOf<Int>()
      for (i in 0 until rawSequence.length()) {
        val index = (rawSequence.opt(i) as? Number)?.toInt() ?: return null
        if (index < 0 || index >= frames.size) return null
        sequence.add(index)
      }
      if (frames.isEmpty() || sequence.isEmpty()) return null
      return PhotoImage(id, name, fit == "remplir", maxOf(0, frameMs), frames, sequence)
    }
  }
}

/**
 * Préférences du widget photo : les données reçues de l'appli et l'image
 * choisie sur chaque widget posé (on passe à la suivante en le touchant).
 */
object PhotoWidgetStore {
  private const val PREFS = "fr.eveetlucas.homewidget.photo"
  private const val KEY_DATA = "data"
  private const val KEY_CHOICE = "choix-"

  private fun prefs(context: Context): SharedPreferences =
    context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

  @Synchronized
  fun saveData(context: Context, json: String) {
    prefs(context).edit().putString(KEY_DATA, json).commit()
  }

  @Synchronized
  fun content(context: Context): PhotoContent? =
    PhotoContent.parse(context, prefs(context).getString(KEY_DATA, null))

  /** Image montrée par ce widget : celle choisie, sinon la plus récente. */
  @Synchronized
  fun imageFor(context: Context, widgetId: Int, content: PhotoContent?): PhotoImage? {
    val images = content?.images.orEmpty()
    val chosen = prefs(context).getString(KEY_CHOICE + widgetId, null)
    return images.firstOrNull { it.id == chosen } ?: images.firstOrNull()
  }

  /** Passe ce widget à l'image suivante (et revient à la première après la dernière). */
  @Synchronized
  fun next(context: Context, widgetId: Int) {
    val images = content(context)?.images.orEmpty()
    if (images.size < 2) return
    val current = imageFor(context, widgetId, PhotoContent(images, 0))
    val index = images.indexOf(current)
    val next = images[(index + 1) % images.size]
    prefs(context).edit().putString(KEY_CHOICE + widgetId, next.id).commit()
  }

  /** Widgets retirés de l'écran d'accueil. */
  @Synchronized
  fun forget(context: Context, widgetIds: IntArray) {
    val editor = prefs(context).edit()
    for (id in widgetIds) editor.remove(KEY_CHOICE + id)
    editor.commit()
  }
}
