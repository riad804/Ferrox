package io.ferrox.photoeditor

import android.app.Activity
import android.content.ContentValues
import android.graphics.Bitmap
import android.net.Uri
import android.os.Bundle
import android.provider.MediaStore
import android.widget.Button
import android.widget.ImageView
import android.widget.Toast
import androidx.activity.result.contract.ActivityResultContracts
import androidx.appcompat.app.AppCompatActivity
import io.ferrox.sdk.ImageSession
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import androidx.lifecycle.lifecycleScope
import java.nio.ByteBuffer

/**
 * Minimal M1 photo editor proving the ferrox SDK end-to-end:
 *   pick → ImageSession(bytes) → chained edits in Rust → render → save JPEG.
 *
 * The original picked bytes are kept so edits are always re-applied from a clean
 * session (each toolbar tap rebuilds the chain — simple + correct for M1).
 */
class MainActivity : AppCompatActivity() {

    private lateinit var imageView: ImageView
    private lateinit var saveButton: Button

    /** Bytes of the originally picked image (the edit source of truth). */
    private var originalBytes: ByteArray? = null

    /** The ordered list of edits to apply on top of the original. */
    private val edits = mutableListOf<(ImageSession) -> Unit>()

    private val pickImage =
        registerForActivityResult(ActivityResultContracts.GetContent()) { uri: Uri? ->
            uri ?: return@registerForActivityResult
            val bytes = contentResolver.openInputStream(uri)?.use { it.readBytes() }
            if (bytes == null) {
                toast("Could not read image")
                return@registerForActivityResult
            }
            originalBytes = bytes
            edits.clear()
            saveButton.isEnabled = true
            render()
        }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        imageView = findViewById(R.id.imageView)
        saveButton = findViewById(R.id.saveButton)

        findViewById<Button>(R.id.pickButton).setOnClickListener { pickImage.launch("image/*") }
        saveButton.setOnClickListener { save() }

        findViewById<Button>(R.id.brightnessButton).setOnClickListener {
            addEdit { it.brightness(20) }
        }
        findViewById<Button>(R.id.contrastButton).setOnClickListener {
            addEdit { it.contrast(1.2f) }
        }
        findViewById<Button>(R.id.grayscaleButton).setOnClickListener {
            addEdit { it.grayscale() }
        }
        findViewById<Button>(R.id.blurButton).setOnClickListener {
            addEdit { it.blur(2.0f) }
        }
        findViewById<Button>(R.id.rotateButton).setOnClickListener {
            addEdit { it.rotate(90u) }
        }
        findViewById<Button>(R.id.cropButton).setOnClickListener {
            addEdit { it.cropCenterSquare() }
        }
        findViewById<Button>(R.id.resetButton).setOnClickListener {
            edits.clear(); render()
        }
    }

    private fun addEdit(edit: (ImageSession) -> Unit) {
        if (originalBytes == null) return
        edits.add(edit)
        render()
    }

    /** Rebuild the edited image off the main thread and show it. */
    private fun render() {
        val src = originalBytes ?: return
        lifecycleScope.launch {
            val bmp = withContext(Dispatchers.Default) {
                // AutoCloseable: the Rust object is freed at the end of `use`.
                ImageSession(src).use { session ->
                    edits.forEach { it(session) }
                    val raw = session.toRgba8()
                    Bitmap.createBitmap(
                        raw.width.toInt(), raw.height.toInt(), Bitmap.Config.ARGB_8888
                    ).apply { copyPixelsFromBuffer(ByteBuffer.wrap(raw.pixels)) }
                }
            }
            imageView.setImageBitmap(bmp)
        }
    }

    /** Export the edited image as JPEG into the device gallery. */
    private fun save() {
        val src = originalBytes ?: return
        lifecycleScope.launch {
            val jpeg = withContext(Dispatchers.Default) {
                ImageSession(src).use { session ->
                    edits.forEach { it(session) }
                    session.exportJpeg(90u)
                }
            }
            val values = ContentValues().apply {
                put(MediaStore.Images.Media.DISPLAY_NAME, "ferrox_${System.currentTimeMillis()}.jpg")
                put(MediaStore.Images.Media.MIME_TYPE, "image/jpeg")
            }
            val uri = contentResolver.insert(
                MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values
            )
            if (uri == null) { toast("Save failed"); return@launch }
            contentResolver.openOutputStream(uri)?.use { it.write(jpeg) }
            toast("Saved to gallery")
        }
    }

    private fun toast(msg: String) =
        Toast.makeText(this, msg, Toast.LENGTH_SHORT).show()
}
