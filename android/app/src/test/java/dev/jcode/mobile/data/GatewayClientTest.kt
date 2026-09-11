package dev.jcode.mobile.data

import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import java.net.ServerSocket
import java.net.Socket
import java.security.MessageDigest
import java.util.Base64
import java.util.concurrent.atomic.AtomicReference
import kotlin.concurrent.thread

/** Local HTTP/websocket fixture exercises the real OkHttp transport, not a mocked client. */
class GatewayClientTest {
    private fun headers(socket: Socket): String {
        val input = socket.getInputStream()
        val bytes = ArrayList<Byte>()
        while (true) {
            val value = input.read()
            check(value >= 0)
            bytes += value.toByte()
            if (bytes.takeLast(4) == listOf(13.toByte(), 10.toByte(), 13.toByte(), 10.toByte())) break
        }
        return bytes.toByteArray().toString(Charsets.UTF_8)
    }
    private fun fixture(block: (Socket) -> Unit, test: (String) -> Unit) {
        ServerSocket(0).use { server ->
            val error = AtomicReference<Throwable>()
            val worker = thread {
                try { server.accept().use { it.soTimeout = 5000; block(it) } } catch (t: Throwable) { error.set(t) }
            }
            test("http://127.0.0.1:${server.localPort}")
            worker.join(6000)
            assertFalse("Fixture did not finish", worker.isAlive)
            error.get()?.let { throw it }
        }
    }
    private fun websocket(socket: Socket, response: String) {
        val request = headers(socket)
        assertTrue(request.contains("Authorization: Bearer test-token", ignoreCase = true))
        assertTrue(request.startsWith("GET /ws HTTP/1.1"))
        assertFalse(request.contains("?token="))
        val key = request.lines().first { it.startsWith("Sec-WebSocket-Key:", true) }.substringAfter(":").trim()
        val accept = Base64.getEncoder().encodeToString(MessageDigest.getInstance("SHA-1").digest((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").toByteArray()))
        socket.getOutputStream().write("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: $accept\r\n\r\n".toByteArray())
        // Wait for client's masked frame before replying.
        val input = socket.getInputStream()
        assertEquals(0x81, input.read())
        val length = input.read() and 127
        assertTrue(length < 126)
        val mask = ByteArray(4); repeat(4) { mask[it] = input.read().toByte() }
        val data = ByteArray(length); repeat(length) { data[it] = (input.read() xor mask[it % 4].toInt()).toByte() }
        assertEquals("list_sessions", org.json.JSONObject(String(data)).getString("type"))
        val payload = response.toByteArray()
        assertTrue(payload.size < 126)
        socket.getOutputStream().write(byteArrayOf(0x81.toByte(), payload.size.toByte()) + payload)
        socket.getOutputStream().flush()
        Thread.sleep(100)
    }
    @Test fun authenticatedOneShotWebsocket() = fixture({ socket ->
        websocket(socket, "{\"type\":\"ack\",\"id\":5}\n{\"type\":\"sessions_list\",\"id\":5,\"sessions\":[]}\n")
    }) { host ->
        val client = GatewayClient()
        try { runBlocking {
            val response = client.exchange(Credential(host, "test-token", "Test"), WireCodec.request("list_sessions", 5), 5)
            assertEquals("sessions_list", response.getString("type"))
        } } finally { client.close() }
    }
    @Test fun oldDaemonGetsActionableUpgradeError() = fixture({ socket ->
        websocket(socket, "{\"type\":\"error\",\"id\":0,\"message\":\"Invalid request\"}\n")
    }) { host ->
        val client = GatewayClient()
        try { runBlocking {
            try { client.exchange(Credential(host, "test-token", "Test"), WireCodec.request("list_sessions", 5), 5); fail("Expected upgrade error") }
            catch (e: GatewayUpgradeException) { assertTrue(e.message!!.contains("Update")) }
        } } finally { client.close() }
    }
    @Test fun revokedPairingIsNotRetried() = fixture({ socket ->
        headers(socket)
        socket.getOutputStream().write("HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".toByteArray())
    }) { host ->
        val client = GatewayClient()
        try { runBlocking {
            try { client.exchange(Credential(host, "test-token", "Test"), WireCodec.request("list_sessions", 5), 5); fail("Expected authentication error") }
            catch (e: GatewayAuthenticationException) { assertTrue(e.message!!.contains("Pair again")) }
        } } finally { client.close() }
    }
    @Test fun pairsWithSixDigitCode() = fixture({ socket ->
        val request = headers(socket)
        assertTrue(request.startsWith("POST /pair HTTP/1.1"))
        val length = request.lines().first { it.startsWith("Content-Length:", true) }.substringAfter(":").trim().toInt()
        val bytes = ByteArray(length); repeat(length) { bytes[it] = socket.getInputStream().read().toByte() }
        val json = org.json.JSONObject(String(bytes))
        assertEquals("012345", json.getString("code"))
        assertEquals("device-test", json.getString("device_id"))
        val body = "{\"token\":\"test-token\",\"server_name\":\"Test host\"}"
        socket.getOutputStream().write("HTTP/1.1 200 OK\r\nContent-Length: ${body.length}\r\nConnection: close\r\n\r\n$body".toByteArray())
    }) { host ->
        val client = GatewayClient()
        try { runBlocking {
            val paired = client.pair(host, "012345", "device-test")
            assertEquals("test-token", paired.token)
            assertEquals("Test host", paired.serverName)
        } } finally { client.close() }
    }
}
