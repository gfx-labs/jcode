package dev.jcode.mobile.data

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class ProviderUsageTest {
    @Test fun missingInvalidAndZeroRemainingStayDistinctAndPartialErrorsSurvive() {
        val result = decodeProviderUsage(JSONObject("""{"type":"mobile_usage","providers":[{"provider":"openai","display_name":"OpenAI","available":true,"error":"Some sources unavailable","limits":[{"name":"missing"},{"name":"invalid","remaining_percent":-1},{"name":"full","remaining_percent":0},{"name":"known","remaining_percent":75}]}]}"""))
        assertEquals(listOf(null, null, 0.0, 75.0), result.providers.single().limits.map { it.remainingPercent })
        assertEquals("Some sources unavailable", result.providers.single().error)
        assertNull(result.fetchedAtSeconds)
    }
    @Test fun unsupportedDaemonsCannotLookLikeZeroRemaining() {
        assertThrows(IllegalArgumentException::class.java) { decodeProviderUsage(JSONObject("""{"type":"error","id":0}""")) }
        val result = decodeProviderUsage(JSONObject("""{"type":"mobile_usage","providers":[],"fetched_at_unix_secs":9223372036854775807}"""))
        assertTrue(result.providers.isEmpty()); assertNull(result.fetchedAtSeconds)
    }
}
