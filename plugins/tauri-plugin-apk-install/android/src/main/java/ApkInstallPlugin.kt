package fr.eveetlucas.apkinstall

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import java.io.File

@InvokeArg
class InstallArgs {
  var path: String? = null
}

/** Ouvre l'installeur d'Android sur l'APK téléchargé par le code Rust. */
@TauriPlugin
class ApkInstallPlugin(private val activity: Activity) : Plugin(activity) {

  @Command
  fun install(invoke: Invoke) {
    val args = invoke.parseArgs(InstallArgs::class.java)
    val dir = File(activity.cacheDir, "maj").canonicalFile
    val apk = args.path?.let { File(it).canonicalFile }
    if (apk == null || apk.parentFile != dir || !apk.name.endsWith(".apk") || !apk.isFile) {
      invoke.reject("APK de mise à jour introuvable")
      return
    }

    // Android 8 et plus : l'appli doit être autorisée à installer des applis.
    // On ouvre le réglage ; la personne revient et appuie à nouveau.
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
        !activity.packageManager.canRequestPackageInstalls()) {
      val settings = Intent(
        Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
        Uri.parse("package:${activity.packageName}"),
      )
      try {
        activity.startActivity(settings)
      } catch (e: Exception) {
        invoke.reject("Impossible d'ouvrir le réglage « Installer des applis inconnues »")
        return
      }
      val result = JSObject()
      result.put("needsPermission", true)
      invoke.resolve(result)
      return
    }

    try {
      val uri = FileProvider.getUriForFile(activity, "${activity.packageName}.majapk", apk)
      val intent = Intent(Intent.ACTION_VIEW)
        .setDataAndType(uri, "application/vnd.android.package-archive")
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      activity.startActivity(intent)
    } catch (e: Exception) {
      invoke.reject("Impossible d'ouvrir l'installeur d'Android : ${e.message}")
      return
    }
    val result = JSObject()
    result.put("needsPermission", false)
    invoke.resolve(result)
  }
}
