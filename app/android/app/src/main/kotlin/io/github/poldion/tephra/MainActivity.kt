package io.github.poldion.tephra

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.provider.OpenableColumns
import android.view.View
import androidx.core.content.FileProvider
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File
import java.security.KeyStore
import kotlin.concurrent.thread

class MainActivity : FlutterActivity() {
    /// Antwort an die Oberfläche, die noch auf einen Dialog des Systems wartet.
    private var pending: MethodChannel.Result? = null

    /// Datei, die nach der Wahl eines Speicherorts dorthin kopiert wird.
    private var saveSource: File? = null

    /// Auftrag, mit dem der Dienst für offene Sitzungen gerade läuft.
    private var keeping: Intent? = null

    /// Ob in diesem Lauf schon nach der Erlaubnis für Benachrichtigungen gefragt wurde.
    private var askedNotifications = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Ohne das zeichnet Android nach Tastatureingaben einen Fokusrahmen um die ganze App.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            findViewById<View>(FLUTTER_VIEW_ID)?.defaultFocusHighlightEnabled = false
        }
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        val messenger = flutterEngine.dartExecutor.binaryMessenger

        // Private Ordner der App: Daten (dort liegt der Tresor) und Zwischendateien.
        MethodChannel(messenger, "tephra/paths").setMethodCallHandler { call, result ->
            when (call.method) {
                "filesDir" -> result.success(filesDir.absolutePath)
                "cacheDir" -> result.success(cacheDir.absolutePath)
                else -> result.notImplemented()
            }
        }

        // Dateien wählen und speichern über die Dialoge des Systems; dafür braucht die App keine Berechtigung.
        MethodChannel(messenger, "tephra/files").setMethodCallHandler { call, result ->
            when (call.method) {
                "pick" -> ask(result, PICK, Intent(Intent.ACTION_OPEN_DOCUMENT).apply {
                    addCategory(Intent.CATEGORY_OPENABLE)
                    type = "*/*"
                })
                "save" -> {
                    saveSource = File(call.argument<String>("path")!!)
                    ask(result, SAVE, Intent(Intent.ACTION_CREATE_DOCUMENT).apply {
                        addCategory(Intent.CATEGORY_OPENABLE)
                        type = "application/octet-stream"
                        putExtra(Intent.EXTRA_TITLE, call.argument<String>("name"))
                    })
                }
                else -> result.notImplemented()
            }
        }

        MethodChannel(messenger, "tephra/system").setMethodCallHandler { call, result ->
            when (call.method) {
                "openUrl" -> {
                    startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(call.argument<String>("url"))))
                    result.success(null)
                }
                // Übergibt eine heruntergeladene APK an die Paketinstallation. Android nimmt sie nur an,
                // wenn sie mit demselben Schlüssel signiert ist wie die installierte App.
                "installApk" -> {
                    val apk = File(call.argument<String>("path")!!)
                    val uri = FileProvider.getUriForFile(this, "$packageName.files", apk)
                    startActivity(Intent(Intent.ACTION_VIEW).apply {
                        setDataAndType(uri, "application/vnd.android.package-archive")
                        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
                    })
                    result.success(null)
                }
                "deviceName" -> result.success(
                    listOf(Build.MANUFACTURER, Build.MODEL)
                        .filter { !it.isNullOrBlank() }
                        .joinToString(" ")
                        .replaceFirstChar { it.uppercase() },
                )
                // Zertifikate, denen das System vertraut, samt der vom Nutzer hinzugefügten: Der Kern
                // prüft damit die Verbindung zum Sync-Server, an den Speicher des Systems kommt er selbst nicht.
                "trustedCertificates" -> thread {
                    try {
                        val store = KeyStore.getInstance("AndroidCAStore").apply { load(null) }
                        val certificates = store.aliases().toList().mapNotNull { store.getCertificate(it)?.encoded }
                        runOnUiThread { result.success(certificates) }
                    } catch (error: Exception) {
                        runOnUiThread { result.error("certificates", error.message, null) }
                    }
                }
                // Entsperren ohne Master-Passwort, siehe DeviceUnlock. Der Keystore kann dauern, deshalb
                // nicht im Hauptthread.
                "deviceUnlockStored", "deviceUnlockSave", "deviceUnlockLoad", "deviceUnlockForget" -> thread {
                    try {
                        val answer: Any? = when (call.method) {
                            "deviceUnlockStored" -> DeviceUnlock.stored(this)
                            "deviceUnlockSave" -> DeviceUnlock.save(this, call.argument<ByteArray>("key")!!)
                            "deviceUnlockLoad" -> DeviceUnlock.load(this)
                            else -> DeviceUnlock.forget(this)
                        }
                        runOnUiThread { result.success(if (answer is Unit) null else answer) }
                    } catch (error: Exception) {
                        runOnUiThread { result.error("deviceUnlock", error.message ?: error.javaClass.simpleName, null) }
                    }
                }
                // Solange Sitzungen offen sind, läuft ein Dienst mit Benachrichtigung; ohne ihn schließt
                // Android die Verbindungen, sobald die App ein paar Sekunden im Hintergrund ist.
                "keepSessions" -> {
                    keepSessions(
                        call.argument<Int>("count") ?: 0,
                        call.argument<String>("title") ?: "Tephra",
                        call.argument<String>("text") ?: "",
                        call.argument<String>("channel") ?: "Tephra",
                    )
                    result.success(null)
                }
                else -> result.notImplemented()
            }
        }
    }

    private fun keepSessions(count: Int, title: String, text: String, channel: String) {
        if (count <= 0) {
            if (keeping == null) return
            keeping = null
            try {
                startService(SessionService.stop(this))
            } catch (error: Exception) {
                stopService(Intent(this, SessionService::class.java))
            }
            return
        }
        val intent = SessionService.show(this, title, text, channel)
        if (!startKeeping(intent)) return
        keeping = intent
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && !askedNotifications) {
            askedNotifications = true
            if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
                requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), NOTIFY)
            }
        }
    }

    private fun startKeeping(intent: Intent): Boolean =
        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) startForegroundService(intent) else startService(intent)
            true
        } catch (error: Exception) {
            // Aus dem Hintergrund lässt Android den Start nicht zu; die Sitzungen laufen dann ohne diesen Schutz.
            false
        }

    override fun onRequestPermissionsResult(requestCode: Int, permissions: Array<out String>, grantResults: IntArray) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        // Erst mit der Erlaubnis erscheint die Benachrichtigung: noch einmal zeigen.
        if (requestCode == NOTIFY) keeping?.let { startKeeping(it) }
    }

    override fun onDestroy() {
        // Endet die App wirklich, enden ihre Sitzungen; dann bleibt auch der Dienst nicht.
        if (isFinishing && keeping != null) {
            keeping = null
            stopService(Intent(this, SessionService::class.java))
        }
        super.onDestroy()
    }

    private fun ask(result: MethodChannel.Result, request: Int, intent: Intent) {
        // Ein noch offener Dialog gilt als abgebrochen.
        pending?.success(null)
        pending = result
        startActivityForResult(intent, request)
    }

    @Deprecated("Die Aktivität von Flutter kennt nur diesen Weg, ein Ergebnis zu erhalten.")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (requestCode != PICK && requestCode != SAVE) {
            @Suppress("DEPRECATION")
            super.onActivityResult(requestCode, resultCode, data)
            return
        }
        val result = pending ?: return
        pending = null
        val uri = data?.data
        if (resultCode != Activity.RESULT_OK || uri == null) {
            result.success(if (requestCode == SAVE) false else null)
            return
        }
        // Kopiert wird abseits der Oberfläche; die Antwort geht danach auf ihrem Thread zurück.
        thread {
            try {
                val answer: Any = if (requestCode == PICK) copyToCache(uri) else copyTo(uri)
                runOnUiThread { result.success(answer) }
            } catch (error: Exception) {
                runOnUiThread { result.error("io", error.message, null) }
            }
        }
    }

    /// Kopiert eine gewählte Datei in den Zwischenordner, wo der Kern sie lesen kann.
    private fun copyToCache(uri: Uri): Map<String, String> {
        var name = "datei"
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
            if (cursor.moveToFirst() && !cursor.isNull(0)) name = cursor.getString(0)
        }
        // Der Name kommt von einer fremden App: Er darf nicht aus dem Ordner hinausführen.
        name = name.substringAfterLast('/').substringAfterLast('\\').ifBlank { "datei" }
        val target = File(File(cacheDir, "upload").apply { mkdirs() }, name)
        contentResolver.openInputStream(uri)!!.use { input -> target.outputStream().use { input.copyTo(it) } }
        return mapOf("path" to target.absolutePath, "name" to name)
    }

    /// Legt die zuvor heruntergeladene Datei am gewählten Ort ab.
    private fun copyTo(uri: Uri): Boolean {
        val source = saveSource ?: return false
        saveSource = null
        contentResolver.openOutputStream(uri)!!.use { output -> source.inputStream().use { it.copyTo(output) } }
        source.delete()
        return true
    }

    private companion object {
        const val PICK = 4201
        const val SAVE = 4202
        const val NOTIFY = 4203
    }
}
