package dev.jcode.mobile.ui

import org.junit.Assert.*
import org.junit.Test

class MarkdownSafetyTest {
    @Test fun deeplyNestedMessagesAvoidRecursiveExtensionVisitorsAndFallbacks() {
        val tree = parseSafeMarkdown("> ".repeat(6000) + "Retain this text")
        assertEquals("Retain this text", boundedMarkdownText(tree))
    }

    @Test fun linksCannotLaunchLocalFilesIntentsOrExecutableSchemes() {
        listOf("javascript:alert(1)", "data:text/html,hello", "file:///sdcard/private", "intent://open", "content://private", "https://user:pass@example.com", "//example.com").forEach {
            assertNull(it, safeMarkdownLink(it))
        }
        assertEquals("https://example.com/docs", safeMarkdownLink("https://example.com/docs"))
        assertEquals("mailto:hello@example.com", safeMarkdownLink("mailto:hello@example.com"))
    }
}
