package fr.eveetlucas.sharetarget

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
class AckSharesArgs {
  var ids: List<String>? = null
}

/** Pont entre le code Rust de l'appli et les partages reçus. */
@TauriPlugin
class ShareTargetPlugin(private val activity: Activity) : Plugin(activity) {

  companion object {
    /** Instance vivante (appli ouverte), pour prévenir le Rust d'un partage. */
    @Volatile
    private var instance: ShareTargetPlugin? = null

    /** Sans effet si l'appli n'est pas ouverte : le partage reste enregistré. */
    fun notifyShare(share: SharedText) {
      val plugin = instance ?: return
      try {
        plugin.trigger("share", share.toJSObject())
      } catch (e: Exception) {
        Log.w(TAG, "Appli non prévenue du partage", e)
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

  @Command
  fun peekShares(invoke: Invoke) {
    val shares = JSArray()
    for (share in ShareStore.peek(activity)) shares.put(share.toJson())
    val result = JSObject()
    result.put("shares", shares)
    invoke.resolve(result)
  }

  @Command
  fun ackShares(invoke: Invoke) {
    val args = invoke.parseArgs(AckSharesArgs::class.java)
    ShareStore.ack(activity, args.ids ?: emptyList())
    invoke.resolve()
  }
}
