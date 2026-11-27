package fr.eveetlucas.homewidget

import android.app.Activity
import android.util.Log
import android.webkit.WebView
import androidx.appcompat.app.AppCompatActivity
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class UpdateArgs {
  var data: String? = null
}

@InvokeArg
class AckActionsArgs {
  var ids: List<String>? = null
}

/**
 * Pont entre le code Rust de l'appli et le widget. Aucune autorisation
 * Android n'est nécessaire.
 */
@TauriPlugin
class HomeWidgetPlugin(private val activity: Activity) : Plugin(activity) {

  companion object {
    /** Instance vivante (appli ouverte), pour prévenir le Rust d'une action. */
    @Volatile
    private var instance: HomeWidgetPlugin? = null

    /** Sans effet si l'appli n'est pas ouverte : l'action reste enregistrée. */
    fun notifyAction(action: WidgetAction) {
      val plugin = instance ?: return
      try {
        plugin.trigger("action", action.toJSObject())
      } catch (e: Exception) {
        Log.w(TAG, "Appli non prévenue de l'action du widget", e)
      }
    }
  }

  override fun load(webView: WebView) {
    super.load(webView)
    instance = this
  }

  override fun onDestroy(activity: AppCompatActivity) {
    if (instance === this) instance = null
    super.onDestroy(activity)
  }

  /** Enregistre les données à afficher et redessine tous les widgets. */
  @Command
  fun update(invoke: Invoke) {
    val args = invoke.parseArgs(UpdateArgs::class.java)
    val data = args.data
    if (data == null) {
      invoke.reject("Données du widget manquantes.")
      return
    }
    WidgetStore.saveData(activity, data)
    try {
      HomeWidgetUpdater.refreshAll(activity)
    } catch (e: Exception) {
      // Les données sont enregistrées : le widget les affichera plus tard.
      Log.w(TAG, "Widget non redessiné", e)
    }
    invoke.resolve()
  }

  /** Enregistre les images du widget photo et redessine ces widgets. */
  @Command
  fun updatePhotos(invoke: Invoke) {
    val args = invoke.parseArgs(UpdateArgs::class.java)
    val data = args.data
    if (data == null) {
      invoke.reject("Données du widget photo manquantes.")
      return
    }
    PhotoWidgetStore.saveData(activity, data)
    try {
      // Lecture des images hors du fil principal.
      PhotoWidgetUpdater.refreshAllAsync(activity)
    } catch (e: Exception) {
      Log.w(TAG, "Widget photo non redessiné", e)
    }
    invoke.resolve()
  }

  @Command
  fun peekActions(invoke: Invoke) {
    val actions = JSArray()
    for (action in WidgetStore.peekActions(activity)) actions.put(action.toJson())
    val result = JSObject()
    result.put("actions", actions)
    invoke.resolve(result)
  }

  @Command
  fun ackActions(invoke: Invoke) {
    val args = invoke.parseArgs(AckActionsArgs::class.java)
    WidgetStore.ackActions(activity, args.ids ?: emptyList())
    invoke.resolve()
  }
}
