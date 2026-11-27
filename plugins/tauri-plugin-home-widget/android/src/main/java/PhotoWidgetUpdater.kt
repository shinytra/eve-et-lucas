package fr.eveetlucas.homewidget

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.util.Log
import android.view.View
import android.widget.RemoteViews
import java.util.concurrent.Executors

/**
 * Dessine le widget photo. Une image fixe va dans une ImageView ; un GIF
 * devient un [android.widget.ViewFlipper] qui fait défiler ses images (déjà
 * réduites et comptées par l'appli : 10 par seconde au plus).
 */
object PhotoWidgetUpdater {
  /** Les images sont lues hors du fil principal, un dessin à la fois. */
  private val executor = Executors.newSingleThreadExecutor()

  /** Marge sous la limite d'Android pour la mémoire des images d'un widget. */
  private const val SAFETY = 0.8

  fun refreshAllAsync(context: Context) {
    val app = context.applicationContext
    val manager = AppWidgetManager.getInstance(app) ?: return
    val ids = manager.getAppWidgetIds(ComponentName(app, PhotoWidgetProvider::class.java))
    if (ids == null || ids.isEmpty()) return
    updateAsync(app, ids, null)
  }

  fun updateAsync(context: Context, ids: IntArray, done: (() -> Unit)?) {
    val app = context.applicationContext
    executor.execute {
      try {
        val manager = AppWidgetManager.getInstance(app)
        if (manager != null) {
          val content = PhotoWidgetStore.content(app)
          for (id in ids) manager.updateAppWidget(id, buildViews(app, id, content))
        }
      } catch (e: Exception) {
        Log.w(TAG, "Widget photo non redessiné", e)
      } finally {
        done?.invoke()
      }
    }
  }

  /**
   * Mémoire permise par Android pour les images d'un widget : 1,5 fois
   * l'écran en ARGB (`AppWidgetServiceImpl`), avec une marge.
   */
  private fun maxBitmapBytes(context: Context): Long {
    val metrics = context.resources.displayMetrics
    return (metrics.widthPixels.toLong() * metrics.heightPixels * 4 * 3 / 2 * SAFETY).toLong()
  }

  private fun buildViews(context: Context, widgetId: Int, content: PhotoContent?): RemoteViews {
    val views = RemoteViews(context.packageName, R.layout.photo_widget)
    val image = PhotoWidgetStore.imageFor(context, widgetId, content)
    val bitmaps = image?.frames?.map { decode(it) }
    if (image == null || bitmaps == null || bitmaps.any { it == null }) {
      val text = when {
        image != null -> R.string.photo_widget_stale
        (content?.pending ?: 0) > 0 -> R.string.photo_widget_pending
        else -> R.string.photo_widget_empty
      }
      views.setTextViewText(R.id.photo_widget_message, context.getString(text))
      views.setViewVisibility(R.id.photo_widget_message, View.VISIBLE)
      views.setViewVisibility(R.id.photo_widget_flipper, View.GONE)
      views.setViewVisibility(R.id.photo_widget_fill, View.GONE)
      views.setViewVisibility(R.id.photo_widget_whole, View.GONE)
      setTap(context, views, widgetId, content)
      return views
    }
    // Petit écran : toutes les images sont réduites pour tenir dans la limite.
    val frames = shrink(bitmaps.filterNotNull(), maxBitmapBytes(context))
    views.setViewVisibility(R.id.photo_widget_message, View.GONE)
    if (image.sequence.size > 1 && image.frameMs > 0) {
      // Animation : une vue par étape ; une même image peut revenir plusieurs
      // fois sans être copiée (RemoteViews ne l'envoie qu'une fois).
      val layout = if (image.fill) R.layout.photo_widget_frame_fill else R.layout.photo_widget_frame_whole
      for (index in image.sequence) {
        val frame = RemoteViews(context.packageName, layout)
        frame.setImageViewBitmap(R.id.photo_widget_frame, frames[index])
        views.addView(R.id.photo_widget_flipper, frame)
      }
      views.setInt(R.id.photo_widget_flipper, "setFlipInterval", image.frameMs)
      views.setContentDescription(R.id.photo_widget_flipper, image.name)
      views.setViewVisibility(R.id.photo_widget_flipper, View.VISIBLE)
      views.setViewVisibility(R.id.photo_widget_fill, View.GONE)
      views.setViewVisibility(R.id.photo_widget_whole, View.GONE)
    } else {
      val target = if (image.fill) R.id.photo_widget_fill else R.id.photo_widget_whole
      val other = if (image.fill) R.id.photo_widget_whole else R.id.photo_widget_fill
      views.setImageViewBitmap(target, frames[image.sequence[0]])
      views.setContentDescription(target, image.name)
      views.setViewVisibility(target, View.VISIBLE)
      views.setViewVisibility(other, View.GONE)
      views.setViewVisibility(R.id.photo_widget_flipper, View.GONE)
    }

    setTap(context, views, widgetId, content)
    return views
  }

  /** Toucher : image suivante s'il y en a plusieurs, sinon ouvrir l'appli. */
  private fun setTap(context: Context, views: RemoteViews, widgetId: Int, content: PhotoContent?) {
    val count = content?.images?.size ?: 0
    if (count > 1) {
      val next = Intent(context, PhotoWidgetReceiver::class.java)
      next.action = PhotoWidgetReceiver.ACTION_NEXT
      next.putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, widgetId)
      val pending = PendingIntent.getBroadcast(
        context,
        widgetId,
        next,
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
      )
      views.setOnClickPendingIntent(R.id.photo_widget_root, pending)
    } else {
      openAppOnTap(context, views)
    }
  }

  private fun openAppOnTap(context: Context, views: RemoteViews) {
    val launch = context.packageManager.getLaunchIntentForPackage(context.packageName) ?: return
    val open = PendingIntent.getActivity(
      context,
      0,
      launch,
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
    )
    views.setOnClickPendingIntent(R.id.photo_widget_root, open)
  }

  private fun decode(path: String): Bitmap? =
    try {
      BitmapFactory.decodeFile(path)
    } catch (e: Exception) {
      Log.w(TAG, "Image du widget photo illisible : $path", e)
      null
    } catch (e: OutOfMemoryError) {
      Log.w(TAG, "Image du widget photo trop grosse : $path", e)
      null
    }

  /** Réduit toutes les images d'autant si elles dépassent la limite. */
  private fun shrink(frames: List<Bitmap>, max: Long): List<Bitmap> {
    val total = frames.sumOf { it.allocationByteCount.toLong() }
    if (total <= max || total == 0L) return frames
    val scale = Math.sqrt(max.toDouble() / total)
    return frames.map {
      val w = maxOf(1, (it.width * scale).toInt())
      val h = maxOf(1, (it.height * scale).toInt())
      Bitmap.createScaledBitmap(it, w, h, true)
    }
  }
}
