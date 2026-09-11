package dev.jcode.mobile.data

import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import okhttp3.*
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONObject
import java.io.IOException
import java.util.concurrent.TimeUnit
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

internal object GatewayAddress {
    fun parse(input: String): HttpUrl {
        val text = input.trim()
        require(text.isNotEmpty()) { "Enter a gateway host" }
        val explicitScheme = text.contains("://")
        val url = (if (explicitScheme) text else "http://$text").toHttpUrl()
        require(url.username.isEmpty() && url.password.isEmpty() && url.query == null && url.fragment == null && url.encodedPath == "/") { "Enter only a host and optional port, not a path or credentials" }
        // An explicit URL honors default HTTP(S) ports. Bare hosts use the Jcode gateway default.
        val authority = text.substringAfter("://")
        val hasPort = if (authority.startsWith("[")) authority.substringAfter("]", "").startsWith(":") else authority.count { it == ':' } == 1
        return if (!explicitScheme && !hasPort) url.newBuilder().port(7643).build() else url
    }
}

internal class GatewayAuthenticationException : IOException("Pairing expired or revoked. Pair again.")
internal class GatewayUpgradeException : IOException("Update the Jcode daemon to a version supporting Android session monitoring.")

internal class GatewayClient {
    private val permits = Semaphore(4)
    private val client = OkHttpClient.Builder().connectTimeout(10, TimeUnit.SECONDS).readTimeout(20, TimeUnit.SECONDS)
        .retryOnConnectionFailure(false).followRedirects(false).followSslRedirects(false).build()

    suspend fun pair(host: String, code: String, deviceId: String): Credential {
        require(code.matches(Regex("[0-9]{6}"))) { "Enter the six-digit pairing code" }
        val base = GatewayAddress.parse(host)
        val body = JSONObject().put("code", code).put("device_id", deviceId).put("device_name", "Jcode Android")
        val request = Request.Builder().url(base.newBuilder().encodedPath("/pair").build())
            .post(body.toString().toRequestBody("application/json".toMediaType())).build()
        val json = suspendCancellableCoroutine<JSONObject> { continuation ->
            val call = client.newCall(request)
            continuation.invokeOnCancellation { call.cancel() }
            call.enqueue(object : Callback {
                override fun onFailure(call: Call, e: IOException) { if (continuation.isActive) continuation.resumeWithException(e) }
                override fun onResponse(call: Call, response: Response) {
                    try {
                        val result = response.use {
                            val parsed = runCatching { JSONObject(it.body?.string().orEmpty()) }.getOrDefault(JSONObject())
                            check(it.isSuccessful) { parsed.text("error").ifBlank { "Pairing failed (HTTP ${it.code})" } }
                            parsed
                        }
                        if (continuation.isActive) continuation.resume(result)
                    } catch (e: Exception) { if (continuation.isActive) continuation.resumeWithException(e) }
                }
            })
        }
        val token = json.text("token")
        check(token.isNotEmpty()) { "Gateway did not return a pairing token" }
        return Credential(base.toString(), token, json.text("server_name").ifBlank { "Jcode" })
    }

    /** Lightweight controls use one socket each. Never subscribe, resume, or take ownership. */
    suspend fun exchange(credential: Credential, payload: String, requestId: Long): JSONObject = permits.withPermit { withTimeout(20_000) {
        suspendCancellableCoroutine { continuation ->
            val request = Request.Builder().url(GatewayAddress.parse(credential.host).newBuilder().encodedPath("/ws").build())
                .header("Authorization", "Bearer ${credential.token}").build()
            val listener = object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) {
                    if (!webSocket.send(payload)) {
                        webSocket.cancel()
                        if (continuation.isActive) continuation.resumeWithException(IOException("Could not send request"))
                    }
                }
                override fun onMessage(webSocket: WebSocket, text: String) {
                    try {
                        for (event in WireCodec.decodeFrame(text)) {
                            if (event.text("type") == "error" && event.optLong("id", -1) == 0L && requestId != 0L) {
                                if (continuation.isActive) continuation.resumeWithException(GatewayUpgradeException())
                                webSocket.close(1000, "Unsupported protocol")
                                return
                            }
                            if (event.optLong("id", -1) == requestId && event.text("type") != "ack" && continuation.isActive) {
                                continuation.resume(event)
                                webSocket.close(1000, "Complete")
                            }
                        }
                    } catch (_: Exception) {
                        if (continuation.isActive) continuation.resumeWithException(IOException("Gateway sent an invalid protocol response"))
                        webSocket.cancel()
                    }
                }
                override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                    if (continuation.isActive) continuation.resumeWithException(
                        if (response?.code == 401) GatewayAuthenticationException() else IOException("Gateway connection failed. Check host and network.", t))
                }
                override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
                    if (continuation.isActive) continuation.resumeWithException(IOException("Gateway closed before confirming the request"))
                }
                override fun onClosing(webSocket: WebSocket, code: Int, reason: String) { webSocket.close(code, null) }
            }
            val socket = client.newWebSocket(request, listener)
            continuation.invokeOnCancellation { socket.cancel() }
        }
    }
    }
    fun close() { client.dispatcher.cancelAll(); client.connectionPool.evictAll() }
}
