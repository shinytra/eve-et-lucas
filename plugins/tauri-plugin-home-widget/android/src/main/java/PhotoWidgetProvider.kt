package fr.eveetlucas.homewidget

import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context

/**
 * Le widget photo. Android l'appelle quand il est posé, après un redémarrage
 * ou une mise à jour de l'appli ; sinon il n'est redessiné que quand l'appli
 * envoie une nouvelle liste d'images ou qu'on le touche. Aucune alarme, aucune
 * mise à jour périodique : un GIF est animé par le lanceur lui-même
 * ([android.widget.ViewFlipper]), qui s'arrête écran éteint ou caché.
 */
class PhotoWidgetProvider : AppWidgetProvider() {
  override fun onUpdate(context: Context, appWidgetManager: AppWidgetManager, appWidgetIds: IntArray) {
    val pending = goAsync()
    PhotoWidgetUpdater.updateAsync(context, appWidgetIds) { pending.finish() }
  }

  override fun onDeleted(context: Context, appWidgetIds: IntArray) {
    PhotoWidgetStore.forget(context, appWidgetIds)
  }
}
