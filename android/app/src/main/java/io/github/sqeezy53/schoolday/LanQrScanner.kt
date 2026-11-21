package io.github.sqeezy53.schoolday

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.ImageFormat
import android.graphics.SurfaceTexture
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.media.ImageReader
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import android.os.SystemClock
import android.view.Surface
import android.view.TextureView
import androidx.compose.foundation.layout.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.zxing.PlanarYUVLuminanceSource
import kotlin.math.abs

@Composable
internal fun LanQrScanner(onClose: () -> Unit, onCode: (String) -> Unit, onError: (String) -> Unit) {
    val close by rememberUpdatedState(onClose)
    val found by rememberUpdatedState(onCode)
    val error by rememberUpdatedState(onError)
    val owner = LocalLifecycleOwner.current
    val context = LocalContext.current
    val scanner = remember(context) { LiveQrCamera(context, { found(it) }, { error(it) }) }
    DisposableEffect(owner, scanner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_STOP) close()
        }
        owner.lifecycle.addObserver(observer)
        onDispose {
            owner.lifecycle.removeObserver(observer)
            scanner.close()
        }
    }
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text("Сканировать QR") },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("Наведите камеру на QR-код School Day на компьютере.")
                AndroidView(
                    factory = { TextureView(it).apply { surfaceTextureListener = scanner } },
                    modifier = Modifier.fillMaxWidth().aspectRatio(1f),
                )
            }
        },
        confirmButton = { TextButton(onClick = onClose) { Text("Закрыть") } },
    )
}

private class LiveQrCamera(
    context: Context,
    private val found: (String) -> Unit,
    private val error: (String) -> Unit,
) : TextureView.SurfaceTextureListener {
    private val manager = context.getSystemService(CameraManager::class.java)
    private val main = Handler(Looper.getMainLooper())
    private var analysis: HandlerThread? = null
    private var camera: CameraDevice? = null
    private var preview: Surface? = null
    private var input: ImageReader? = null
    private var closed = false

    @SuppressLint("MissingPermission")
    override fun onSurfaceTextureAvailable(texture: SurfaceTexture, width: Int, height: Int) {
        if (closed || preview != null) return
        try {
            val id = manager.cameraIdList.firstOrNull {
                manager.getCameraCharacteristics(it).get(CameraCharacteristics.LENS_FACING) ==
                    CameraCharacteristics.LENS_FACING_BACK
            } ?: manager.cameraIdList.firstOrNull() ?: return failed()
            val formats = manager.getCameraCharacteristics(id)
                .get(CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP) ?: return failed()
            val scanSize = formats.getOutputSizes(ImageFormat.YUV_420_888)
                ?.filter { it.width <= 1280 && it.height <= 960 }
                ?.minByOrNull { abs(it.width - 640) + abs(it.height - 480) } ?: return failed()
            val previewSize = formats.getOutputSizes(SurfaceTexture::class.java)
                ?.minByOrNull { abs(it.width - 640) + abs(it.height - 480) } ?: return failed()
            texture.setDefaultBufferSize(previewSize.width, previewSize.height)
            val surface = Surface(texture).also { preview = it }
            val reader = ImageReader.newInstance(scanSize.width, scanSize.height, ImageFormat.YUV_420_888, 2)
                .also { input = it }
            val thread = HandlerThread("School Day QR").apply { start() }.also { analysis = it }
            var nextFrame = 0L
            reader.setOnImageAvailableListener({ frames ->
                runCatching {
                    frames.acquireLatestImage()?.use { image ->
                        val now = SystemClock.elapsedRealtime()
                        if (now < nextFrame) return@use
                        nextFrame = now + 120
                        val plane = image.planes.first()
                        val buffer = plane.buffer
                        val pixels = ByteArray(image.width * image.height)
                        for (row in 0 until image.height) {
                            buffer.position(row * plane.rowStride)
                            buffer.get(pixels, row * image.width, image.width)
                        }
                        val source = PlanarYUVLuminanceSource(pixels, image.width, image.height,
                            0, 0, image.width, image.height, false)
                        decodeInvitation(source)?.let { code ->
                            main.post {
                                if (!closed) {
                                    close()
                                    found(code)
                                }
                            }
                        }
                    }
                }
            }, Handler(thread.looper))
            manager.openCamera(id, object : CameraDevice.StateCallback() {
                override fun onOpened(device: CameraDevice) {
                    if (closed) {
                        device.close()
                        return
                    }
                    camera = device
                    startPreview(device, surface, reader.surface)
                }

                override fun onDisconnected(device: CameraDevice) {
                    device.close()
                    failed()
                }

                override fun onError(device: CameraDevice, reason: Int) {
                    device.close()
                    failed()
                }
            }, main)
        } catch (_: Exception) {
            failed()
        }
    }

    @Suppress("DEPRECATION")
    private fun startPreview(device: CameraDevice, surface: Surface, frames: Surface) {
        try {
            device.createCaptureSession(listOf(surface, frames), object : CameraCaptureSession.StateCallback() {
                override fun onConfigured(session: CameraCaptureSession) {
                    if (closed) {
                        session.close()
                        return
                    }
                    try {
                        val request = device.createCaptureRequest(CameraDevice.TEMPLATE_PREVIEW)
                        request.addTarget(surface)
                        request.addTarget(frames)
                        session.setRepeatingRequest(request.build(), null, main)
                    } catch (_: Exception) {
                        failed()
                    }
                }

                override fun onConfigureFailed(session: CameraCaptureSession) {
                    session.close()
                    failed()
                }
            }, main)
        } catch (_: Exception) {
            failed()
        }
    }

    private fun failed() {
        if (closed) return
        close()
        error("Камера недоступна. Введите код с компьютера.")
    }

    fun close() {
        if (closed) return
        closed = true
        camera?.close()
        preview?.release()
        val reader = input
        val thread = analysis
        if (reader != null && thread != null) {
            Handler(thread.looper).post { reader.close() }
            thread.quitSafely()
        } else {
            reader?.close()
        }
    }

    override fun onSurfaceTextureDestroyed(texture: SurfaceTexture): Boolean {
        close()
        return true
    }

    override fun onSurfaceTextureSizeChanged(texture: SurfaceTexture, width: Int, height: Int) = Unit
    override fun onSurfaceTextureUpdated(texture: SurfaceTexture) = Unit
}
