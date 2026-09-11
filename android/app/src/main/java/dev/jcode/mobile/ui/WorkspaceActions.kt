package dev.jcode.mobile.ui

/** UI actions stay separate from the network/lifecycle owner for adaptive testing. */
internal data class WorkspaceActions(
    val selectSession: (String) -> Unit = {},
    val refresh: () -> Unit = {},
    val sendMessage: (String, String) -> Unit = { _, _ -> },
    val broadcast: (List<String>, String) -> Unit = { _, _ -> },
    val dismissError: () -> Unit = {},
    val togglePin: (String) -> Unit = {},
    val renameSession: (String, String) -> Unit = { _, _ -> },
    val loadDemo: () -> Unit = {},
)
