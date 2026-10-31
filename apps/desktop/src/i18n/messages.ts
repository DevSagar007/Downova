// UI copy catalogue. English is the reference; every locale must define every key
// (enforced by the `Messages` type). Keep strings short, plain and status-first (design.md §9).
export const en = {
  "app.name": "Downova",
  "app.skipToContent": "Skip to download list",
  "app.version": "Version 0.1.0 — development build",

  "unavailable.service":
    "Unavailable: this browser preview cannot reach the background download service. Use the Downova desktop app.",
  "unavailable.checking": "Unavailable: still checking the download service.",
  "unavailable.disconnected":
    "Unavailable: the download service is not connected. Use Reconnect above the list.",
  "unavailable.incompatible":
    "Unavailable: a different version of the download service is running. Quit the other Downova version first.",
  "unavailable.notWired":
    "Unavailable: the download service is running, but this command is connected to it in a later milestone.",
  "unavailable.noSelection": "Unavailable: select a download in the list first.",
  "unavailable.busy": "Unavailable: wait for the previous command to finish.",
  "unavailable.finished": "Unavailable: this download is already complete.",
  "unavailable.cancelled": "Unavailable: this download was cancelled.",
  "unavailable.failed": "Unavailable: this download has stopped with an error.",
  "unavailable.failedResume":
    "Unavailable: a download that stopped with an error cannot be resumed. Use Retry to start it again from the beginning.",
  "unavailable.notPaused":
    "Unavailable: this download is not paused. If it is waiting, it continues by itself.",
  "unavailable.alreadyPaused": "Unavailable: this download is already paused.",

  "unavailable.desktopOnly":
    "Unavailable: only the Downova desktop app can stop the background download service.",
  "tray.open": "Open Downova",

  "table.progressOf": "Progress of {name}",
  "cancel.title": "Cancel this download?",
  "cancel.body":
    "Downova stops the transfer. It cannot be resumed afterwards; adding its link again starts over. Nothing is deleted: any partial data stays in the download folder as a .downova-part file.",
  "cancel.confirm": "Cancel download",
  "cancel.keep": "Keep downloading",
  "cancel.pending": "Cancelling…",
  "cancel.failed": "The download was not cancelled",
  "remove.title": "Remove this download from the list?",
  "remove.body":
    "It disappears from the list, also after a restart. The downloaded file and any partial data stay where they are; nothing is deleted.",
  "remove.confirm": "Remove from list",
  "remove.keep": "Keep in list",
  "remove.pending": "Removing…",
  "remove.failed": "The download was not removed",
  "clear.title": "Clear finished downloads?",
  "clear.body":
    "This removes {count} completed, failed and cancelled downloads from the list, including any hidden by the current category or search. Files are kept. Unfinished and paused downloads are not touched. Removed downloads stay out of the list after a restart.",
  "clear.confirm": "Clear finished",
  "clear.keep": "Keep them",
  "clear.pending": "Clearing…",
  "clear.none": "Unavailable: there are no completed, failed or cancelled downloads to clear.",
  "clear.failedTitle": "Some downloads were not cleared",
  "clear.result": "{removed} of {total} cleared; {failed} kept.",
  "clear.announce": "{count} finished downloads cleared from the list. Files were kept.",
  "command.uncertainState":
    "The download service did not confirm in time. The list shows whether it took effect.",
  "announce.completed": "{name}: download complete.",
  "announce.failed": "{name}: download stopped with an error.",
  "announce.cancelled": "{name}: download cancelled.",
  "announce.removed": "{name} removed from the list. The file was kept.",
  "options.closeBehaviour": "When the window is closed",
  "options.close.hide_to_tray": "Hide to the tray or menu bar (downloads continue)",
  "options.close.close_window": "Close the window (downloads continue)",
  "options.close.note":
    "Downloads keep running in the background service either way. Use Exit and stop downloads to stop them.",
  "options.close.noTray":
    "No tray or menu bar icon is available here, so hiding minimizes the window instead. Downloads keep running either way.",
  "options.close.unavailable": "Only the Downova desktop app has this setting.",
  "options.close.saveFailed": "The setting could not be saved. The previous one still applies.",
  "exit.title": "Exit and stop downloads?",
  "exit.body":
    "Running downloads are paused and saved, and the background service stops. They stay paused until you resume them. Completed downloads are not affected.",
  "exit.confirm": "Exit and stop downloads",
  "exit.cancel": "Cancel",
  "exit.stopping": "Stopping…",
  "exit.stopped": "The download service has stopped. Downova is closing.",
  "exit.stillRunning":
    "The download service confirmed the stop and paused the downloads, but it is still running. Try again in a moment.",
  "exit.failed": "Downloads were not stopped",
  "exit.uncertain":
    "The download service did not confirm in time. It may be stopping; check the list before trying again.",
  "exit.unavailable": "The download service could not be reached. Nothing was stopped.",
  "unavailable.remove":
    "Unavailable: deleting downloaded files is not available. Remove from list (File menu) keeps the file.",
  "unavailable.notFinished":
    "Unavailable: only a completed, failed or cancelled download can be removed from the list. Cancel it first.",
  "unavailable.finalizing":
    "Unavailable: this download is already being placed at its destination and can no longer be cancelled.",
  "unavailable.updates":
    "Unavailable: signed updates are not set up yet (planned for the public beta).",

  "menu.label": "Application menu",
  "menu.tasks": "Tasks",
  "menu.file": "File",
  "menu.downloads": "Downloads",
  "menu.view": "View",
  "menu.help": "Help",
  "menu.tasks.addUrl": "Add new download…",
  "menu.tasks.addBatch": "Add batch download from clipboard",
  "menu.tasks.import": "Import download list…",
  "menu.tasks.exit": "Exit and stop downloads",
  "menu.file.resume": "Resume download",
  "menu.file.stop": "Stop download",
  "menu.file.cancel": "Cancel download…",
  "menu.file.remove": "Remove from list…",
  "menu.file.clearFinished": "Clear finished…",
  "menu.file.redownload": "Retry…",
  "menu.file.properties": "Properties",
  "menu.downloads.stopAll": "Stop all",
  "menu.downloads.deleteCompleted": "Delete all completed",
  "menu.downloads.scheduler": "Scheduler…",
  "menu.downloads.startQueue": "Start queue",
  "menu.downloads.stopQueue": "Stop queue",
  "menu.downloads.options": "Options…",
  "menu.downloads.speedLimiter": "Speed limiter…",
  "menu.view.categories": "Show categories",
  "menu.view.themeSystem": "Theme: match system",
  "menu.view.themeLight": "Theme: light",
  "menu.view.themeDark": "Theme: dark",
  "menu.view.langEn": "English",
  "menu.view.langBn": "বাংলা (Bangla)",
  "menu.help.about": "About Downova",
  "menu.help.updates": "Check for updates…",

  "toolbar.label": "Download commands",
  "toolbar.addUrl": "Add URL",
  "toolbar.resume": "Resume",
  "toolbar.stop": "Stop",
  "toolbar.stopAll": "Stop all",
  "toolbar.delete": "Delete",
  "toolbar.deleteCompleted": "Delete completed",
  "toolbar.options": "Options",
  "toolbar.scheduler": "Scheduler",
  "toolbar.startQueue": "Start queue",
  "toolbar.stopQueue": "Stop queue",

  "categories.label": "Categories",
  "categories.all": "All downloads",
  "categories.compressed": "Compressed",
  "categories.documents": "Documents",
  "categories.music": "Music",
  "categories.programs": "Programs",
  "categories.video": "Video",
  "categories.unfinished": "Unfinished",
  "categories.finished": "Finished",
  "categories.queues": "Queues",
  "categories.mainQueue": "Main download queue",
  "categories.expand": "Expand",
  "categories.collapse": "Collapse",
  "categories.hide": "Hide categories",

  "table.caption": "Downloads in {category}",
  "table.fileName": "File name",
  "table.size": "Size",
  "table.status": "Status",
  "table.timeLeft": "Time left",
  "table.rate": "Transfer rate",
  "table.lastTry": "Last try",
  "table.description": "Description",
  "table.empty": "No downloads in this category.",
  "table.emptyDisconnected":
    "The download list comes from the background service, which this window cannot reach.",
  "table.emptyHint": "Use Add URL to download a file.",
  "table.noMatches": "No downloads match your search.",
  "table.noMatchesHint": "Clear the search to see all downloads in this category.",
  "table.loading": "Loading downloads…",
  "table.unknownSize": "Unknown",

  "search.label": "Search downloads",
  "search.placeholder": "Search by name or site",
  "search.clear": "Clear search",
  "search.matchesShort": "Matches: {count}",
  "search.noMatchesShort": "No matches",
  "search.matchCount": "{count} downloads match your search.",
  "search.none": "No downloads match your search.",

  "status.queued": "Queued",
  "status.probing": "Connecting",
  "status.transferring": "Downloading {percent}",
  "status.verifying": "Verifying",
  "status.scanning": "Scanning",
  "status.finalizing": "Finishing",
  "status.completed": "Complete",
  "status.failed": "Error",
  "status.cancelled": "Cancelled",
  "status.paused": "Paused",
  "status.waiting": "Waiting: {reason}",
  "blocker.offline": "offline",
  "blocker.metered_network": "metered network",
  "blocker.schedule_window": "schedule",
  "blocker.rate_limit_cooldown": "server asked to wait",
  "blocker.disk_unavailable": "disk unavailable",
  "blocker.volume_missing": "drive disconnected",
  "blocker.queue_limit": "queue limit",

  // Error catalogue (CORE-11), keyed by the contract's ErrorKind and NextAction wire names.
  // Clients never parse the service's English `detail`; these strings say what the user can do.
  "error.kind.NETWORK_OFFLINE": "The connection to the server failed or was interrupted.",
  "error.kind.AUTH_REQUIRED": "The server or a proxy requires a sign-in.",
  "error.kind.LINK_REFRESH_REQUIRED":
    "The server refused the link or no longer has the file. The link may have expired, been revoked or need a sign-in.",
  "error.kind.REMOTE_CHANGED":
    "The file on the server changed after the download started, so the saved part cannot be used.",
  "error.kind.RANGE_UNSUPPORTED":
    "The server cannot send part of a file, so the download cannot continue where it stopped.",
  "error.kind.INVALID_RANGE":
    "The server sent data that does not fit what was asked for, or the file has the wrong length. It was not used.",
  "error.kind.CHECKSUM_MISMATCH":
    "The downloaded file does not match the expected checksum. It was deleted, not saved.",
  "error.kind.DISK_FULL": "The destination drive does not have enough free space.",
  "error.kind.VOLUME_MISSING": "The destination drive or folder is not available.",
  "error.kind.DESTINATION_UNWRITABLE":
    "The file cannot be saved in the download folder: the name is not allowed there, there is no permission, the drive is read-only, or another file already has the name.",
  "error.kind.HOST_NOT_INSTALLED": "The browser cannot reach Downova's helper program.",
  "error.kind.SCAN_BLOCKED": "A security scan blocked this file.",
  "error.kind.BACKEND_CAPABILITY_UNAVAILABLE": "The download engine cannot do this safely.",
  "error.kind.SERVER_BUSY": "The server is busy or is limiting requests.",
  "error.kind.TLS_HANDSHAKE_FAILED":
    "A secure connection could not be made: the site's certificate is not trusted, has expired or does not match the site.",
  "error.kind.SERVICE_UNAVAILABLE":
    "The Downova background service is not running or did not respond.",
  "error.kind.INVALID_REQUEST": "The request or the server's response could not be accepted.",
  "error.kind.IDEMPOTENCY_CONFLICT": "This request was already used for a different download.",
  "error.kind.INTERNAL": "Downova ran into an unexpected problem.",
  "error.action.retry": "Try again in a moment.",
  "error.action.wait": "Wait a while before trying again.",
  "error.action.refresh_link": "Get a new link from the website and use Refresh link.",
  "error.action.sign_in":
    "Downova cannot sign in or use browser cookies yet. Download this file in your browser instead.",
  "error.action.free_disk_space": "Free up space on the destination drive.",
  "error.action.reconnect_volume": "Reconnect the drive or restore the download folder.",
  "error.action.install_desktop_app": "Install the Downova desktop app.",
  "error.action.choose_restart_or_keep":
    "The saved part cannot be continued safely, so the file has to be downloaded again from the start.",
  "error.action.contact_support": "Please report this problem to the Downova developers.",
  "error.followUp.failed": "This download has stopped. Retry starts it again from the beginning.",
  "error.followUp.continues": "The download continues by itself once this is fixed.",
  "error.followUp.autoRetry": "Downova will try again automatically.",

  "statusbar.label": "Status",
  "statusbar.checking": "Checking the download service…",
  "statusbar.disconnected": "Download service not connected",
  "statusbar.connected": "Connected to download service",
  "statusbar.count": "{count} downloads",

  "options.title": "Options",
  "options.close": "Close",
  "options.done": "Done",
  "options.language": "Language",
  "options.theme": "Theme",
  "options.theme.system": "Match system",
  "options.theme.light": "Light",
  "options.theme.dark": "Dark",
  "options.localNote":
    "These preferences are stored only in this window's local storage until the settings service exists.",
  "language.en": "English",
  "language.bn": "বাংলা (Bangla)",

  "about.title": "About Downova",
  "about.body":
    "A free, account-free download manager. This is a development build: downloads run in a background service and keep going when this window closes.",

  "service.staleBody":
    "Showing the last known list. It may be out of date until the service is reconnected.",
  "service.downBody": "The download list and commands need the background service.",
  "service.incompatibleTitle": "A different version of the download service is running",
  "service.reconnect": "Reconnect",

  "addUrl.title": "Add new download",
  "addUrl.label": "Address (URL)",
  "addUrl.hint": "An http or https link to one file.",
  "addUrl.submit": "Add",
  "addUrl.adding": "Adding…",
  "addUrl.cancel": "Cancel",
  "addUrl.invalid.empty": "Enter a link.",
  "addUrl.invalid.tooLong": "This link is too long.",
  "addUrl.invalid.spaces": "The link contains spaces or control characters.",
  "addUrl.invalid.scheme": "Only links that start with http:// or https:// can be added.",
  "addUrl.invalid.credentials":
    "Links with a user name or password in them are not accepted. Remove the part before @.",
  "addUrl.invalid.host": "The link has no server name.",
  "addUrl.uncertain":
    "The download service did not confirm in time, so this link may already have been added. Select Add again to check: the same request is sent, so it is never added twice.",
  "addUrl.unavailable": "The download service could not be reached. Nothing was added.",
  "addUrl.rejected": "The download service did not add this link.",

  "command.pauseFailed": "Stop did not complete",
  "command.resumeFailed": "Resume did not complete",
  "command.uncertain":
    "The download service did not confirm in time. The list shows whether it took effect.",
  "command.unavailable": "The download service could not be reached. Nothing changed.",

  "verification.local_hash": "SHA-256 computed locally (no publisher checksum)",
  "verification.publisher_checksum": "Publisher checksum matched",

  // B2.8–B2.11
  "blocker.queue_stopped": "queue stopped",
  "blocker.name_conflict": "name taken — confirm",
  "error.kind.DUPLICATE_DOWNLOAD": "This link is already in the download list.",
  "categories.other": "Other files",
  "service.reconnecting": "Trying to reconnect to the download service…",
  "addUrl.category": "Save as",
  "addUrl.category.auto": "Automatic (from the file type)",
  "addUrl.queue": "Queue",
  "addUrl.checking": "Checking…",
  "duplicate.title": "This link is already in the list",
  "duplicate.body": "Adding it again creates a second download of the same file.",
  "duplicate.more": "and {count} more",
  "duplicate.show": "Show existing download",
  "duplicate.again": "Download again",
  "duplicate.cancel": "Cancel",
  "options.downloads": "Downloads",
  "options.folders": "Download folders",
  "options.folders.note":
    "Each type of file is saved in its folder. Use a full folder path that already exists, or leave a field empty to use the default download folder. A change applies to new downloads only; existing downloads stay where they are.",
  "options.collision": "When a file with the same name exists",
  "options.collision.rename": "Save with a new name (recommended)",
  "options.collision.ask": "Ask me first",
  "options.collision.note": "Downova never replaces an existing file.",
  "options.service.save": "Save download settings",
  "options.service.saving": "Saving…",
  "options.service.saved": "Download settings saved.",
  "options.service.saveFailed": "The download settings were not saved.",
  "options.service.unavailable":
    "Download settings can be changed while the download service is running.",
  "nameConflict.title": "A file with this name already exists",
  "nameConflict.body":
    "Nothing was replaced. Downova will save this download as {name}, or you can cancel it.",
  "nameConflict.confirm": "Save as {name}",
  "nameConflict.pending": "Saving…",
  "nameConflict.failed": "The new name was not confirmed.",
  "menu.downloads.manageQueues": "Manage queues…",
  "menu.file.queue": "Queue and priority…",
  "menu.file.moveUp": "Move up in queue",
  "menu.file.moveDown": "Move down in queue",
  "menu.file.moveTop": "Move to top of queue",
  "menu.file.moveBottom": "Move to bottom of queue",
  "unavailable.notQueued": "Unavailable: a finished download has no place in a queue.",
  "unavailable.queueRunning": "Unavailable: {name} is already running.",
  "unavailable.queueStopped": "Unavailable: {name} is already stopped.",
  "announce.queueStarted": "{name} started",
  "announce.queueStopped": "{name} stopped",
  "announce.moved": "{name} moved",
  "queues.title": "Manage queues",
  "queues.list": "Queues",
  "queues.name": "Name",
  "queues.limit": "Downloads at once",
  "queues.status.running": "running",
  "queues.status.stopped": "stopped",
  "queues.new": "New queue name",
  "queues.create": "Add queue",
  "queues.rename": "Rename",
  "queues.saveLimit": "Set limit",
  "queues.delete": "Delete queue",
  "queues.deleteTo": "Move its downloads to",
  "queues.deleteNote": "Deleting a queue never deletes downloads or files.",
  "queues.mainNote": "The main queue cannot be deleted.",
  "queues.failed": "The queue change was not saved.",
  "queues.saved": "Saved.",
  "jobQueue.title": "Queue and priority",
  "jobQueue.queue": "Queue",
  "jobQueue.priority": "Priority",
  "jobQueue.priorityNote":
    "Higher priority downloads start first when a place is free; a running download is never stopped for it.",
  "jobQueue.order": "Order in queue",
  "jobQueue.failed": "The change was not saved.",
  "priority.low": "Low",
  "priority.normal": "Normal",
  "priority.high": "High",

  // B2.12
  "history.heading": "History",
  "history.keep": "Keep finished downloads in the list",
  "history.forever": "Forever (recommended)",
  "history.days": "For {count} days",
  "history.year": "For 1 year",
  "history.note":
    "Completed, failed and cancelled downloads leave the list this long after they finished. Downloaded files and partial (.downova-part) files always stay on disk. Unfinished, paused and waiting downloads are never removed.",
  "history.apply": "Apply",
  "history.unchanged": "Unavailable: this is already the saved setting.",
  "history.clean": "Clean now",
  "history.cleanForever":
    "Unavailable: finished downloads are kept forever. Choose a period and apply it first.",
  "history.confirmTitle": "Remove finished downloads from the list?",
  "history.confirmBody":
    "{count} of {total} finished downloads are older than this period and will leave the list now.",
  "history.confirmNone": "No finished download is old enough to leave the list now.",
  "history.filesStay": "Their files stay on disk. This only removes list entries.",
  "history.confirmApply": "Apply and remove",
  "history.confirmClean": "Remove from list",
  "history.keepThem": "Keep them",
  "history.saved": "History setting saved.",
  "history.cleaned":
    "{count} finished downloads were removed from the list. Their files stay on disk.",
  "history.kept":
    "{count} could not be removed now and stay in the list; they are tried again later.",
  "history.failed": "The history setting was not changed.",
  "history.unavailable": "History settings can be changed while the download service is running.",

  // B2.16–B2.19: Retry, Refresh link, checksum, folders
  "addUrl.sha256": "Publisher's SHA-256 (optional)",
  "addUrl.sha256.hint":
    "From the publisher's download page. When given, the download only completes if the file matches it.",
  "addUrl.sha256.length": "A SHA-256 checksum is exactly 64 characters long.",
  "addUrl.sha256.digits": "A SHA-256 checksum contains only the digits 0–9 and the letters a–f.",
  "unavailable.notFailed":
    "Unavailable: only a download that stopped with an error can be retried.",
  "unavailable.needsNewLink":
    "Unavailable: the link no longer works. Use Refresh link with a new link instead.",
  "unavailable.needsSignIn":
    "Unavailable: the server asks for a sign-in, which Downova does not support yet. Download this file in your browser.",
  "unavailable.pauseFirst": "Unavailable: pause the download before replacing its link.",
  "menu.file.refreshLink": "Refresh link…",
  "menu.file.openFolder": "Open containing folder",
  "retry.title": "Retry this download?",
  "retry.body":
    "Retry starts this download again from the beginning, with the same name, folder, queue and priority.",
  "retry.discard":
    "The {size} already downloaded is discarded: a download that stopped with an error has no point it can safely continue from.",
  "retry.checksum":
    "The publisher's SHA-256 given with the link is checked again before it completes.",
  "retry.confirm": "Retry from the beginning",
  "retry.keep": "Keep as it is",
  "retry.pending": "Retrying…",
  "retry.failed": "Retry did not start",
  "retry.uncertain":
    "The download service did not confirm in time. Select Retry again: the same request is sent, so the download never starts twice.",
  "refresh.title": "Refresh link",
  "refresh.label": "New link",
  "refresh.hint":
    "A new link to the same file, for example from the website's download page. The progress is kept only if Downova can prove it is the same file.",
  "refresh.submit": "Use new link",
  "refresh.cancel": "Cancel",
  "refresh.pending": "Checking the new link…",
  "refresh.kept": "The new link serves the same file. The download continues from where it was.",
  "refresh.restarted": "The new link is used. The download starts again from the beginning.",
  "refresh.choiceTitle": "The progress cannot be kept",
  "refresh.reason.no_checkpoint": "There is no saved progress that could be continued.",
  "refresh.reason.validator_missing":
    "The server gives no version tag (ETag) that proves it is the same file.",
  "refresh.reason.validator_changed": "The server says it is a different version of the file.",
  "refresh.reason.length_changed": "The file has a different size.",
  "refresh.reason.range_ignored": "The server cannot continue a download part-way.",
  "refresh.reason.content_differs": "The data already downloaded differs from the new link's file.",
  "refresh.reason.different_resource":
    "It is a different address, and no publisher checksum can prove the finished file.",
  "refresh.reason.unverifiable": "The new link could not be checked.",
  "refresh.restart": "Restart from zero with the new link",
  "refresh.keepCurrent": "Keep current download",
  "refresh.restartNote":
    "Restart discards the {size} already downloaded for this download only. Keep current changes nothing.",
  "refresh.failed": "The link was not replaced",
  "refresh.uncertain":
    "The download service did not confirm in time. Select the button again: the same request is sent, so nothing happens twice.",
  "openFolder.fileMissing": "The folder opened, but the file is no longer in it.",
  "openFolder.failed": "The folder could not be opened",
  "details.checksumPublisher":
    "Publisher checksum matched: the file is exactly what the publisher's SHA-256 describes.",
  "details.checksumLocal":
    "SHA-256 computed locally. No publisher checksum was given, so this records the file's fingerprint but does not prove it is the publisher's file.",
  "details.checksumPending":
    "A publisher SHA-256 was given: the file must match it before the download completes.",
  "details.sha256": "SHA-256",
  "details.checksumMismatch":
    "Check the SHA-256 on the publisher's page. If it is right, the received file is not the publisher's: use Retry to download it again, or Refresh link with a link from the publisher.",
  "details.retry": "Retry…",
  "details.refreshLink": "Refresh link…",
  "details.openFolder": "Open containing folder",
  "announce.retried": "{name} starts again from the beginning.",
  "announce.linkKept": "The link of {name} was replaced; the download continues from where it was.",
  "announce.linkRestarted":
    "The link of {name} was replaced; the download starts from the beginning.",
  "options.browse": "Browse…",
  "options.browseFor": "Choose the folder for {category}",
  "options.browseFailed": "The folder dialog could not be opened. Type the folder instead.",
  "options.browseDesktopOnly":
    "Unavailable: only the Downova desktop app can show the folder dialog.",
} as const;

export type MessageKey = keyof typeof en;
export type Messages = Record<MessageKey, string>;

export const bn: Messages = {
  "app.name": "Downova",
  "app.skipToContent": "ডাউনলোড তালিকায় যান",
  "app.version": "সংস্করণ 0.1.0 — development build",

  "unavailable.service":
    "এখন ব্যবহার করা যাবে না: এই ব্রাউজার প্রিভিউ ব্যাকগ্রাউন্ড ডাউনলোড সার্ভিসে পৌঁছাতে পারে না। Downova ডেস্কটপ অ্যাপ ব্যবহার করুন।",
  "unavailable.checking": "এখন ব্যবহার করা যাবে না: ডাউনলোড সার্ভিস এখনো যাচাই করা হচ্ছে।",
  "unavailable.disconnected":
    "এখন ব্যবহার করা যাবে না: ডাউনলোড সার্ভিস যুক্ত নেই। তালিকার ওপরের «আবার যুক্ত করুন» ব্যবহার করুন।",
  "unavailable.incompatible":
    "এখন ব্যবহার করা যাবে না: ডাউনলোড সার্ভিসের অন্য একটি সংস্করণ চলছে। আগে অন্য Downova সংস্করণটি বন্ধ করুন।",
  "unavailable.notWired":
    "এখন ব্যবহার করা যাবে না: ডাউনলোড সার্ভিস চলছে, কিন্তু এই কমান্ডটি পরের ধাপে সার্ভিসের সাথে যুক্ত হবে।",
  "unavailable.noSelection": "এখন ব্যবহার করা যাবে না: আগে তালিকা থেকে একটি ডাউনলোড নির্বাচন করুন।",
  "unavailable.busy": "এখন ব্যবহার করা যাবে না: আগের কমান্ডটি শেষ হওয়া পর্যন্ত অপেক্ষা করুন।",
  "unavailable.finished": "এখন ব্যবহার করা যাবে না: এই ডাউনলোডটি আগেই সম্পূর্ণ হয়েছে।",
  "unavailable.cancelled": "এখন ব্যবহার করা যাবে না: এই ডাউনলোডটি বাতিল করা হয়েছে।",
  "unavailable.failed": "এখন ব্যবহার করা যাবে না: এই ডাউনলোডটি একটি ত্রুটির কারণে থেমে গেছে।",
  "unavailable.failedResume":
    "অনুপলব্ধ: ত্রুটিতে থেমে যাওয়া ডাউনলোড আবার চালু করা যায় না। শুরু থেকে আবার নামাতে আবার চেষ্টা ব্যবহার করুন।",
  "unavailable.notPaused":
    "এখন ব্যবহার করা যাবে না: এই ডাউনলোডটি থামানো নেই। এটি অপেক্ষায় থাকলে নিজে থেকেই চলবে।",
  "unavailable.alreadyPaused": "এখন ব্যবহার করা যাবে না: এই ডাউনলোডটি আগেই থামানো আছে।",

  "unavailable.desktopOnly":
    "এখন ব্যবহার করা যাবে না: শুধু Downova ডেস্কটপ অ্যাপ ব্যাকগ্রাউন্ড ডাউনলোড সার্ভিস বন্ধ করতে পারে।",
  "tray.open": "Downova খুলুন",

  "table.progressOf": "{name}-এর অগ্রগতি",
  "cancel.title": "এই ডাউনলোডটি বাতিল করবেন?",
  "cancel.body":
    "Downova ট্রান্সফারটি থামাবে। এরপর এটি আর চালু করা যাবে না; লিংকটি আবার যোগ করলে শুরু থেকে নামবে। কিছুই মুছে ফেলা হবে না: আংশিক ডেটা ডাউনলোড ফোল্ডারে .downova-part ফাইল হিসেবে থাকবে।",
  "cancel.confirm": "ডাউনলোড বাতিল করুন",
  "cancel.keep": "ডাউনলোড চালিয়ে যান",
  "cancel.pending": "বাতিল করা হচ্ছে…",
  "cancel.failed": "ডাউনলোড বাতিল হয়নি",
  "remove.title": "এই ডাউনলোডটি তালিকা থেকে সরাবেন?",
  "remove.body":
    "এটি তালিকা থেকে চলে যাবে, রিস্টার্টের পরেও। ডাউনলোড করা ফাইল ও আংশিক ডেটা যেখানে আছে সেখানেই থাকবে; কিছুই মুছে ফেলা হবে না।",
  "remove.confirm": "তালিকা থেকে সরান",
  "remove.keep": "তালিকায় রাখুন",
  "remove.pending": "সরানো হচ্ছে…",
  "remove.failed": "ডাউনলোডটি সরানো হয়নি",
  "clear.title": "সম্পন্ন ডাউনলোডগুলো মুছবেন?",
  "clear.body":
    "এতে {count}টি সম্পূর্ণ, ব্যর্থ ও বাতিল ডাউনলোড তালিকা থেকে সরে যাবে, বর্তমান ক্যাটাগরি বা খোঁজে লুকানোগুলোও। ফাইলগুলো রাখা হবে। অসম্পূর্ণ ও থামানো ডাউনলোডে হাত দেওয়া হবে না। সরানো ডাউনলোড রিস্টার্টের পরেও তালিকায় থাকবে না।",
  "clear.confirm": "সম্পন্নগুলো মুছুন",
  "clear.keep": "রাখুন",
  "clear.pending": "মোছা হচ্ছে…",
  "clear.none": "এখন ব্যবহার করা যাবে না: মুছতে কোনো সম্পূর্ণ, ব্যর্থ বা বাতিল ডাউনলোড নেই।",
  "clear.failedTitle": "কিছু ডাউনলোড মোছা যায়নি",
  "clear.result": "{total}টির মধ্যে {removed}টি মোছা হয়েছে; {failed}টি রাখা হয়েছে।",
  "clear.announce": "{count}টি সম্পন্ন ডাউনলোড তালিকা থেকে সরানো হয়েছে। ফাইলগুলো রাখা হয়েছে।",
  "command.uncertainState":
    "ডাউনলোড সার্ভিস সময়মতো নিশ্চিত করেনি। এটি কার্যকর হয়েছে কি না, তালিকায় দেখা যাবে।",
  "announce.completed": "{name}: ডাউনলোড সম্পূর্ণ।",
  "announce.failed": "{name}: ত্রুটির কারণে ডাউনলোড থেমে গেছে।",
  "announce.cancelled": "{name}: ডাউনলোড বাতিল হয়েছে।",
  "announce.removed": "{name} তালিকা থেকে সরানো হয়েছে। ফাইলটি রাখা হয়েছে।",
  "options.closeBehaviour": "উইন্ডো বন্ধ করলে",
  "options.close.hide_to_tray": "ট্রে বা মেনু বারে লুকিয়ে রাখুন (ডাউনলোড চলতে থাকবে)",
  "options.close.close_window": "উইন্ডো বন্ধ করুন (ডাউনলোড চলতে থাকবে)",
  "options.close.note":
    "দুই ক্ষেত্রেই ডাউনলোড ব্যাকগ্রাউন্ড সার্ভিসে চলতে থাকে। থামাতে «বন্ধ করুন ও ডাউনলোড থামান» ব্যবহার করুন।",
  "options.close.noTray":
    "এখানে ট্রে বা মেনু বারের আইকন নেই, তাই লুকানোর বদলে উইন্ডোটি ছোট (minimize) করা হয়। দুই ক্ষেত্রেই ডাউনলোড চলতে থাকে।",
  "options.close.unavailable": "এই সেটিং শুধু Downova ডেস্কটপ অ্যাপে আছে।",
  "options.close.saveFailed": "সেটিংটি সংরক্ষণ করা যায়নি। আগেরটিই কার্যকর আছে।",
  "exit.title": "বন্ধ করবেন ও ডাউনলোড থামাবেন?",
  "exit.body":
    "চলমান ডাউনলোডগুলো থামিয়ে সংরক্ষণ করা হবে এবং ব্যাকগ্রাউন্ড সার্ভিস বন্ধ হবে। আপনি আবার চালু না করা পর্যন্ত এগুলো থামানো থাকবে। সম্পূর্ণ হওয়া ডাউনলোডে কোনো প্রভাব পড়বে না।",
  "exit.confirm": "বন্ধ করুন ও ডাউনলোড থামান",
  "exit.cancel": "বাতিল",
  "exit.stopping": "বন্ধ করা হচ্ছে…",
  "exit.stopped": "ডাউনলোড সার্ভিস বন্ধ হয়েছে। Downova বন্ধ হচ্ছে।",
  "exit.stillRunning":
    "ডাউনলোড সার্ভিস বন্ধের অনুরোধ নিশ্চিত করেছে এবং ডাউনলোড থামিয়েছে, কিন্তু এখনো চলছে। একটু পরে আবার চেষ্টা করুন।",
  "exit.failed": "ডাউনলোড থামানো হয়নি",
  "exit.uncertain":
    "ডাউনলোড সার্ভিস সময়মতো নিশ্চিত করেনি। এটি হয়তো বন্ধ হচ্ছে; আবার চেষ্টার আগে তালিকাটি দেখুন।",
  "exit.unavailable": "ডাউনলোড সার্ভিসে পৌঁছানো যায়নি। কিছুই থামানো হয়নি।",
  "unavailable.remove":
    "এখন ব্যবহার করা যাবে না: ডাউনলোড করা ফাইল মুছে ফেলার সুবিধা নেই। «তালিকা থেকে সরান» (ফাইল মেনু) ফাইলটি রেখে দেয়।",
  "unavailable.notFinished":
    "এখন ব্যবহার করা যাবে না: শুধু সম্পূর্ণ, ব্যর্থ বা বাতিল হওয়া ডাউনলোড তালিকা থেকে সরানো যায়। আগে এটি বাতিল করুন।",
  "unavailable.finalizing":
    "এখন ব্যবহার করা যাবে না: এই ডাউনলোডটি গন্তব্যে রাখা হচ্ছে, তাই এখন আর বাতিল করা যায় না।",
  "unavailable.updates":
    "এখন ব্যবহার করা যাবে না: স্বাক্ষরিত আপডেট এখনো চালু হয়নি (public beta-তে আসবে)।",

  "menu.label": "অ্যাপ্লিকেশন মেনু",
  "menu.tasks": "কাজ",
  "menu.file": "ফাইল",
  "menu.downloads": "ডাউনলোড",
  "menu.view": "দেখুন",
  "menu.help": "সাহায্য",
  "menu.tasks.addUrl": "নতুন ডাউনলোড যোগ করুন…",
  "menu.tasks.addBatch": "ক্লিপবোর্ড থেকে একাধিক ডাউনলোড যোগ করুন",
  "menu.tasks.import": "ডাউনলোড তালিকা আমদানি করুন…",
  "menu.tasks.exit": "বন্ধ করুন ও ডাউনলোড থামান",
  "menu.file.resume": "ডাউনলোড চালু করুন",
  "menu.file.stop": "ডাউনলোড থামান",
  "menu.file.cancel": "ডাউনলোড বাতিল করুন…",
  "menu.file.remove": "তালিকা থেকে সরান…",
  "menu.file.clearFinished": "সম্পন্নগুলো মুছুন…",
  "menu.file.redownload": "আবার চেষ্টা…",
  "menu.file.properties": "বৈশিষ্ট্য",
  "menu.downloads.stopAll": "সব থামান",
  "menu.downloads.deleteCompleted": "সম্পন্ন সব মুছুন",
  "menu.downloads.scheduler": "সময়সূচি…",
  "menu.downloads.startQueue": "কিউ শুরু করুন",
  "menu.downloads.stopQueue": "কিউ থামান",
  "menu.downloads.options": "অপশন…",
  "menu.downloads.speedLimiter": "গতি সীমা…",
  "menu.view.categories": "ক্যাটাগরি দেখান",
  "menu.view.themeSystem": "থিম: সিস্টেমের মতো",
  "menu.view.themeLight": "থিম: হালকা",
  "menu.view.themeDark": "থিম: গাঢ়",
  "menu.view.langEn": "English",
  "menu.view.langBn": "বাংলা (Bangla)",
  "menu.help.about": "Downova সম্পর্কে",
  "menu.help.updates": "আপডেট খুঁজুন…",

  "toolbar.label": "ডাউনলোড কমান্ড",
  "toolbar.addUrl": "URL যোগ",
  "toolbar.resume": "চালু",
  "toolbar.stop": "থামান",
  "toolbar.stopAll": "সব থামান",
  "toolbar.delete": "মুছুন",
  "toolbar.deleteCompleted": "সম্পন্ন মুছুন",
  "toolbar.options": "অপশন",
  "toolbar.scheduler": "সময়সূচি",
  "toolbar.startQueue": "কিউ শুরু",
  "toolbar.stopQueue": "কিউ থামান",

  "categories.label": "ক্যাটাগরি",
  "categories.all": "সব ডাউনলোড",
  "categories.compressed": "সংকুচিত",
  "categories.documents": "ডকুমেন্ট",
  "categories.music": "গান",
  "categories.programs": "প্রোগ্রাম",
  "categories.video": "ভিডিও",
  "categories.unfinished": "অসম্পূর্ণ",
  "categories.finished": "সম্পূর্ণ",
  "categories.queues": "কিউ",
  "categories.mainQueue": "প্রধান ডাউনলোড কিউ",
  "categories.expand": "খুলুন",
  "categories.collapse": "বন্ধ করুন",
  "categories.hide": "ক্যাটাগরি লুকান",

  "table.caption": "{category}-এর ডাউনলোড",
  "table.fileName": "ফাইলের নাম",
  "table.size": "আকার",
  "table.status": "অবস্থা",
  "table.timeLeft": "বাকি সময়",
  "table.rate": "গতি",
  "table.lastTry": "শেষ চেষ্টা",
  "table.description": "বিবরণ",
  "table.empty": "এই ক্যাটাগরিতে কোনো ডাউনলোড নেই।",
  "table.emptyDisconnected":
    "ডাউনলোডের তালিকা ব্যাকগ্রাউন্ড সার্ভিস থেকে আসে, যেখানে এই উইন্ডো পৌঁছাতে পারছে না।",
  "table.emptyHint": "কোনো ফাইল ডাউনলোড করতে «URL যোগ» ব্যবহার করুন।",
  "table.noMatches": "আপনার খোঁজের সাথে কোনো ডাউনলোড মেলে না।",
  "table.noMatchesHint": "এই ক্যাটাগরির সব ডাউনলোড দেখতে খোঁজটি মুছে ফেলুন।",
  "table.loading": "ডাউনলোডের তালিকা আনা হচ্ছে…",
  "table.unknownSize": "অজানা",

  "search.label": "ডাউনলোড খুঁজুন",
  "search.placeholder": "নাম বা সাইট দিয়ে খুঁজুন",
  "search.clear": "খোঁজ মুছুন",
  "search.matchesShort": "মিল: {count}",
  "search.noMatchesShort": "কোনো মিল নেই",
  "search.matchCount": "আপনার খোঁজের সাথে {count}টি ডাউনলোড মেলে।",
  "search.none": "আপনার খোঁজের সাথে কোনো ডাউনলোড মেলে না।",

  "status.queued": "অপেক্ষমাণ",
  "status.probing": "সংযোগ হচ্ছে",
  "status.transferring": "ডাউনলোড হচ্ছে {percent}",
  "status.verifying": "যাচাই হচ্ছে",
  "status.scanning": "স্ক্যান হচ্ছে",
  "status.finalizing": "শেষ করা হচ্ছে",
  "status.completed": "সম্পূর্ণ",
  "status.failed": "ত্রুটি",
  "status.cancelled": "বাতিল",
  "status.paused": "থামানো",
  "status.waiting": "অপেক্ষা: {reason}",
  "blocker.offline": "ইন্টারনেট নেই",
  "blocker.metered_network": "মিটারড নেটওয়ার্ক",
  "blocker.schedule_window": "সময়সূচি",
  "blocker.rate_limit_cooldown": "সার্ভার অপেক্ষা করতে বলেছে",
  "blocker.disk_unavailable": "ডিস্ক পাওয়া যাচ্ছে না",
  "blocker.volume_missing": "ড্রাইভ বিচ্ছিন্ন",
  "blocker.queue_limit": "কিউ সীমা",

  "error.kind.NETWORK_OFFLINE": "সার্ভারের সাথে সংযোগ ব্যর্থ হয়েছে বা মাঝপথে বিচ্ছিন্ন হয়েছে।",
  "error.kind.AUTH_REQUIRED": "সার্ভার বা প্রক্সি সাইন-ইন চাইছে।",
  "error.kind.LINK_REFRESH_REQUIRED":
    "সার্ভার লিংকটি গ্রহণ করেনি বা ফাইলটি সেখানে আর নেই। লিংকের মেয়াদ শেষ হয়ে থাকতে পারে, লিংকটি বাতিল হয়ে থাকতে পারে, বা সাইন-ইন লাগতে পারে।",
  "error.kind.REMOTE_CHANGED":
    "ডাউনলোড শুরুর পর সার্ভারের ফাইলটি বদলে গেছে, তাই সংরক্ষিত অংশটি ব্যবহার করা যাবে না।",
  "error.kind.RANGE_UNSUPPORTED":
    "সার্ভার ফাইলের আংশিক অংশ পাঠাতে পারে না, তাই যেখানে থেমেছিল সেখান থেকে ডাউনলোড চালিয়ে নেওয়া যাবে না।",
  "error.kind.INVALID_RANGE":
    "সার্ভারের পাঠানো ডেটা চাওয়া অংশের সাথে মেলে না, বা ফাইলের দৈর্ঘ্য ভুল। এই ডেটা ব্যবহার করা হয়নি।",
  "error.kind.CHECKSUM_MISMATCH":
    "ডাউনলোড করা ফাইলটি প্রত্যাশিত checksum-এর সাথে মেলেনি। ফাইলটি সংরক্ষণ না করে মুছে ফেলা হয়েছে।",
  "error.kind.DISK_FULL": "গন্তব্য ড্রাইভে যথেষ্ট খালি জায়গা নেই।",
  "error.kind.VOLUME_MISSING": "গন্তব্য ড্রাইভ বা ফোল্ডার পাওয়া যাচ্ছে না।",
  "error.kind.DESTINATION_UNWRITABLE":
    "ডাউনলোড ফোল্ডারে ফাইলটি সংরক্ষণ করা যাচ্ছে না: নামটি সেখানে গ্রহণযোগ্য নয়, অনুমতি নেই, ড্রাইভটি শুধু পড়ার জন্য (read-only), অথবা এই নামে আরেকটি ফাইল আগে থেকেই আছে।",
  "error.kind.HOST_NOT_INSTALLED":
    "ব্রাউজার Downova-র সহায়ক প্রোগ্রামের সাথে যোগাযোগ করতে পারছে না।",
  "error.kind.SCAN_BLOCKED": "নিরাপত্তা স্ক্যান এই ফাইলটি আটকে দিয়েছে।",
  "error.kind.BACKEND_CAPABILITY_UNAVAILABLE": "ডাউনলোড ইঞ্জিন এটি নিরাপদে করতে পারে না।",
  "error.kind.SERVER_BUSY": "সার্ভার ব্যস্ত, অথবা অনুরোধের সংখ্যা সীমিত করছে।",
  "error.kind.TLS_HANDSHAKE_FAILED":
    "নিরাপদ সংযোগ তৈরি করা যায়নি: সাইটের সার্টিফিকেট বিশ্বস্ত নয়, মেয়াদোত্তীর্ণ, অথবা সাইটের সাথে মেলে না।",
  "error.kind.SERVICE_UNAVAILABLE": "Downova-র ব্যাকগ্রাউন্ড সার্ভিস চলছে না বা সাড়া দেয়নি।",
  "error.kind.INVALID_REQUEST": "অনুরোধটি বা সার্ভারের উত্তরটি গ্রহণ করা যায়নি।",
  "error.kind.IDEMPOTENCY_CONFLICT":
    "এই অনুরোধটি আগেই অন্য একটি ডাউনলোডের জন্য ব্যবহার করা হয়েছে।",
  "error.kind.INTERNAL": "Downova একটি অপ্রত্যাশিত সমস্যায় পড়েছে।",
  "error.action.retry": "একটু পরে আবার চেষ্টা করুন।",
  "error.action.wait": "কিছুক্ষণ অপেক্ষা করে আবার চেষ্টা করুন।",
  "error.action.refresh_link": "ওয়েবসাইট থেকে নতুন লিংক নিন এবং লিংক বদলান ব্যবহার করুন।",
  "error.action.sign_in":
    "Downova এখনো সাইন-ইন করতে বা ব্রাউজারের কুকি ব্যবহার করতে পারে না। ফাইলটি বরং ব্রাউজারে ডাউনলোড করুন।",
  "error.action.free_disk_space": "গন্তব্য ড্রাইভে জায়গা খালি করুন।",
  "error.action.reconnect_volume": "ড্রাইভটি আবার যুক্ত করুন বা ডাউনলোড ফোল্ডারটি ফিরিয়ে আনুন।",
  "error.action.install_desktop_app": "Downova ডেস্কটপ অ্যাপ ইনস্টল করুন।",
  "error.action.choose_restart_or_keep":
    "সংরক্ষিত অংশটি নিরাপদে চালিয়ে নেওয়া যাবে না, তাই ফাইলটি শুরু থেকে আবার ডাউনলোড করতে হবে।",
  "error.action.contact_support": "অনুগ্রহ করে সমস্যাটি Downova-র ডেভেলপারদের জানান।",
  "error.followUp.failed": "এই ডাউনলোড থেমে গেছে। আবার চেষ্টা করলে এটি শুরু থেকে আবার নামবে।",
  "error.followUp.continues": "সমস্যাটি ঠিক হলে ডাউনলোড নিজে থেকেই চলবে।",
  "error.followUp.autoRetry": "Downova নিজে থেকেই আবার চেষ্টা করবে।",

  "statusbar.label": "অবস্থা",
  "statusbar.checking": "ডাউনলোড সার্ভিস যাচাই করা হচ্ছে…",
  "statusbar.disconnected": "ডাউনলোড সার্ভিস যুক্ত নয়",
  "statusbar.connected": "ডাউনলোড সার্ভিসে যুক্ত",
  "statusbar.count": "{count}টি ডাউনলোড",

  "options.title": "অপশন",
  "options.close": "বন্ধ করুন",
  "options.done": "ঠিক আছে",
  "options.language": "ভাষা",
  "options.theme": "থিম",
  "options.theme.system": "সিস্টেমের মতো",
  "options.theme.light": "হালকা",
  "options.theme.dark": "গাঢ়",
  "options.localNote":
    "সেটিংস সার্ভিস তৈরি না হওয়া পর্যন্ত এই পছন্দগুলো শুধু এই উইন্ডোর local storage-এ থাকে।",
  "language.en": "English",
  "language.bn": "বাংলা (Bangla)",

  "about.title": "Downova সম্পর্কে",
  "about.body":
    "বিনামূল্যের, অ্যাকাউন্ট ছাড়া ব্যবহারযোগ্য ডাউনলোড ম্যানেজার। এটি একটি development build: ডাউনলোড একটি ব্যাকগ্রাউন্ড সার্ভিসে চলে এবং এই উইন্ডো বন্ধ করলেও চলতে থাকে।",

  "service.staleBody":
    "শেষ জানা তালিকাটি দেখানো হচ্ছে। সার্ভিস আবার যুক্ত না হওয়া পর্যন্ত এটি পুরোনো হতে পারে।",
  "service.downBody": "ডাউনলোডের তালিকা ও কমান্ডের জন্য ব্যাকগ্রাউন্ড সার্ভিস দরকার।",
  "service.incompatibleTitle": "ডাউনলোড সার্ভিসের অন্য একটি সংস্করণ চলছে",
  "service.reconnect": "আবার যুক্ত করুন",

  "addUrl.title": "নতুন ডাউনলোড যোগ করুন",
  "addUrl.label": "ঠিকানা (URL)",
  "addUrl.hint": "একটি ফাইলের http বা https লিংক।",
  "addUrl.submit": "যোগ করুন",
  "addUrl.adding": "যোগ করা হচ্ছে…",
  "addUrl.cancel": "বাতিল",
  "addUrl.invalid.empty": "একটি লিংক লিখুন।",
  "addUrl.invalid.tooLong": "লিংকটি খুব লম্বা।",
  "addUrl.invalid.spaces": "লিংকে ফাঁকা জায়গা বা নিয়ন্ত্রণ-অক্ষর আছে।",
  "addUrl.invalid.scheme": "শুধু http:// বা https:// দিয়ে শুরু হওয়া লিংক যোগ করা যায়।",
  "addUrl.invalid.credentials":
    "ব্যবহারকারীর নাম বা পাসওয়ার্ডসহ লিংক গ্রহণ করা হয় না। @ চিহ্নের আগের অংশটি সরিয়ে দিন।",
  "addUrl.invalid.host": "লিংকে কোনো সার্ভারের নাম নেই।",
  "addUrl.uncertain":
    "ডাউনলোড সার্ভিস সময়মতো নিশ্চিত করেনি, তাই লিংকটি হয়তো আগেই যোগ হয়ে গেছে। যাচাই করতে আবার «যোগ করুন» নির্বাচন করুন: একই অনুরোধ পাঠানো হয়, তাই এটি কখনো দুবার যোগ হয় না।",
  "addUrl.unavailable": "ডাউনলোড সার্ভিসে পৌঁছানো যায়নি। কিছুই যোগ করা হয়নি।",
  "addUrl.rejected": "ডাউনলোড সার্ভিস এই লিংকটি যোগ করেনি।",

  "command.pauseFailed": "থামানো সম্পন্ন হয়নি",
  "command.resumeFailed": "আবার চালু করা সম্পন্ন হয়নি",
  "command.uncertain":
    "ডাউনলোড সার্ভিস সময়মতো নিশ্চিত করেনি। এটি কার্যকর হয়েছে কি না, তালিকায় দেখা যাবে।",
  "command.unavailable": "ডাউনলোড সার্ভিসে পৌঁছানো যায়নি। কিছুই বদলায়নি।",

  "verification.local_hash": "SHA-256 এখানে গণনা করা (প্রকাশকের চেকসাম নেই)",
  "verification.publisher_checksum": "প্রকাশকের চেকসাম মিলেছে",

  // B2.8–B2.11
  "blocker.queue_stopped": "কিউ থামানো",
  "blocker.name_conflict": "নাম আগেই আছে — নিশ্চিত করুন",
  "error.kind.DUPLICATE_DOWNLOAD": "এই লিংকটি ডাউনলোড তালিকায় আগে থেকেই আছে।",
  "categories.other": "অন্যান্য ফাইল",
  "service.reconnecting": "ডাউনলোড সার্ভিসের সাথে আবার সংযোগের চেষ্টা চলছে…",
  "addUrl.category": "যেভাবে সংরক্ষণ হবে",
  "addUrl.category.auto": "স্বয়ংক্রিয় (ফাইলের ধরন দেখে)",
  "addUrl.queue": "কিউ",
  "addUrl.checking": "যাচাই চলছে…",
  "duplicate.title": "এই লিংকটি তালিকায় আগে থেকেই আছে",
  "duplicate.body": "আবার যোগ করলে একই ফাইলের দ্বিতীয় একটি ডাউনলোড তৈরি হবে।",
  "duplicate.more": "এবং আরও {count}টি",
  "duplicate.show": "আগের ডাউনলোডটি দেখান",
  "duplicate.again": "আবার ডাউনলোড করুন",
  "duplicate.cancel": "বাতিল",
  "options.downloads": "ডাউনলোড",
  "options.folders": "ডাউনলোড ফোল্ডার",
  "options.folders.note":
    "প্রতিটি ধরনের ফাইল তার নিজের ফোল্ডারে সংরক্ষিত হয়। আগে থেকে থাকা কোনো ফোল্ডারের পূর্ণ পাথ দিন, অথবা ডিফল্ট ডাউনলোড ফোল্ডার ব্যবহার করতে ঘরটি খালি রাখুন। পরিবর্তন শুধু নতুন ডাউনলোডে প্রযোজ্য; আগের ডাউনলোডগুলো যেখানে আছে সেখানেই থাকে।",
  "options.collision": "একই নামের ফাইল থাকলে",
  "options.collision.rename": "নতুন নামে সংরক্ষণ করুন (প্রস্তাবিত)",
  "options.collision.ask": "আগে আমাকে জিজ্ঞাসা করুন",
  "options.collision.note": "Downova কখনো আগের কোনো ফাইল প্রতিস্থাপন করে না।",
  "options.service.save": "ডাউনলোড সেটিংস সংরক্ষণ করুন",
  "options.service.saving": "সংরক্ষণ হচ্ছে…",
  "options.service.saved": "ডাউনলোড সেটিংস সংরক্ষিত হয়েছে।",
  "options.service.saveFailed": "ডাউনলোড সেটিংস সংরক্ষিত হয়নি।",
  "options.service.unavailable": "ডাউনলোড সার্ভিস চালু থাকলে ডাউনলোড সেটিংস বদলানো যায়।",
  "nameConflict.title": "এই নামে একটি ফাইল আগে থেকেই আছে",
  "nameConflict.body":
    "কিছুই প্রতিস্থাপন করা হয়নি। Downova এই ডাউনলোডটি {name} নামে সংরক্ষণ করবে, অথবা আপনি এটি বাতিল করতে পারেন।",
  "nameConflict.confirm": "{name} নামে সংরক্ষণ করুন",
  "nameConflict.pending": "সংরক্ষণ হচ্ছে…",
  "nameConflict.failed": "নতুন নামটি নিশ্চিত হয়নি।",
  "menu.downloads.manageQueues": "কিউ পরিচালনা…",
  "menu.file.queue": "কিউ ও অগ্রাধিকার…",
  "menu.file.moveUp": "কিউতে উপরে সরান",
  "menu.file.moveDown": "কিউতে নিচে সরান",
  "menu.file.moveTop": "কিউয়ের শুরুতে সরান",
  "menu.file.moveBottom": "কিউয়ের শেষে সরান",
  "unavailable.notQueued": "অনুপলব্ধ: শেষ হওয়া ডাউনলোডের কিউতে কোনো জায়গা নেই।",
  "unavailable.queueRunning": "অনুপলব্ধ: {name} আগে থেকেই চলছে।",
  "unavailable.queueStopped": "অনুপলব্ধ: {name} আগে থেকেই থামানো।",
  "announce.queueStarted": "{name} চালু হয়েছে",
  "announce.queueStopped": "{name} থেমেছে",
  "announce.moved": "{name} সরানো হয়েছে",
  "queues.title": "কিউ পরিচালনা",
  "queues.list": "কিউসমূহ",
  "queues.name": "নাম",
  "queues.limit": "একসাথে কয়টি ডাউনলোড",
  "queues.status.running": "চলছে",
  "queues.status.stopped": "থামানো",
  "queues.new": "নতুন কিউয়ের নাম",
  "queues.create": "কিউ যোগ করুন",
  "queues.rename": "নাম বদলান",
  "queues.saveLimit": "সীমা ঠিক করুন",
  "queues.delete": "কিউ মুছুন",
  "queues.deleteTo": "এর ডাউনলোডগুলো সরান",
  "queues.deleteNote": "কিউ মুছলে কোনো ডাউনলোড বা ফাইল মোছে না।",
  "queues.mainNote": "প্রধান কিউ মোছা যায় না।",
  "queues.failed": "কিউয়ের পরিবর্তন সংরক্ষিত হয়নি।",
  "queues.saved": "সংরক্ষিত হয়েছে।",
  "jobQueue.title": "কিউ ও অগ্রাধিকার",
  "jobQueue.queue": "কিউ",
  "jobQueue.priority": "অগ্রাধিকার",
  "jobQueue.priorityNote":
    "জায়গা খালি হলে বেশি অগ্রাধিকারের ডাউনলোড আগে শুরু হয়; এর জন্য চলমান কোনো ডাউনলোড থামানো হয় না।",
  "jobQueue.order": "কিউতে ক্রম",
  "jobQueue.failed": "পরিবর্তনটি সংরক্ষিত হয়নি।",
  "priority.low": "কম",
  "priority.normal": "স্বাভাবিক",
  "priority.high": "বেশি",

  // B2.12
  "history.heading": "ইতিহাস",
  "history.keep": "শেষ হওয়া ডাউনলোড তালিকায় রাখুন",
  "history.forever": "সবসময় (প্রস্তাবিত)",
  "history.days": "{count} দিন",
  "history.year": "১ বছর",
  "history.note":
    "সম্পূর্ণ, ব্যর্থ ও বাতিল ডাউনলোড শেষ হওয়ার এতদিন পর তালিকা থেকে সরে যায়। ডাউনলোড করা ফাইল ও আংশিক (.downova-part) ফাইল সবসময় ডিস্কে থাকে। অসম্পূর্ণ, থামানো বা অপেক্ষমাণ ডাউনলোড কখনো সরানো হয় না।",
  "history.apply": "প্রয়োগ করুন",
  "history.unchanged": "অনুপলব্ধ: এটিই সংরক্ষিত সেটিং।",
  "history.clean": "এখনই পরিষ্কার করুন",
  "history.cleanForever":
    "অনুপলব্ধ: শেষ হওয়া ডাউনলোড সবসময় রাখা হচ্ছে। আগে একটি সময়সীমা বেছে প্রয়োগ করুন।",
  "history.confirmTitle": "শেষ হওয়া ডাউনলোড তালিকা থেকে সরাবেন?",
  "history.confirmBody":
    "{total}টি শেষ হওয়া ডাউনলোডের মধ্যে {count}টি এই সময়সীমার চেয়ে পুরোনো, সেগুলো এখনই তালিকা থেকে সরে যাবে।",
  "history.confirmNone": "এখন তালিকা থেকে সরানোর মতো পুরোনো কোনো শেষ হওয়া ডাউনলোড নেই।",
  "history.filesStay": "এগুলোর ফাইল ডিস্কে থাকবে। শুধু তালিকার এন্ট্রি সরানো হয়।",
  "history.confirmApply": "প্রয়োগ করে সরান",
  "history.confirmClean": "তালিকা থেকে সরান",
  "history.keepThem": "রেখে দিন",
  "history.saved": "ইতিহাস সেটিং সংরক্ষিত হয়েছে।",
  "history.cleaned":
    "{count}টি শেষ হওয়া ডাউনলোড তালিকা থেকে সরানো হয়েছে। এগুলোর ফাইল ডিস্কে আছে।",
  "history.kept": "{count}টি এখন সরানো যায়নি এবং তালিকায় আছে; পরে আবার চেষ্টা করা হবে।",
  "history.failed": "ইতিহাস সেটিং বদলানো হয়নি।",
  "history.unavailable": "ডাউনলোড সার্ভিস চালু থাকলে ইতিহাস সেটিং বদলানো যায়।",

  // B2.16–B2.19
  "addUrl.sha256": "প্রকাশকের SHA-256 (ঐচ্ছিক)",
  "addUrl.sha256.hint":
    "প্রকাশকের ডাউনলোড পাতা থেকে নিন। দিলে ফাইলটি এর সাথে মিললে তবেই ডাউনলোড শেষ হয়।",
  "addUrl.sha256.length": "SHA-256 চেকসাম ঠিক ৬৪ অক্ষরের হয়।",
  "addUrl.sha256.digits": "SHA-256 চেকসামে শুধু ০–৯ অঙ্ক এবং a–f অক্ষর থাকে।",
  "unavailable.notFailed": "অনুপলব্ধ: শুধু ত্রুটিতে থেমে যাওয়া ডাউনলোড আবার চেষ্টা করা যায়।",
  "unavailable.needsNewLink":
    "অনুপলব্ধ: লিংকটি আর কাজ করে না। এর বদলে নতুন লিংক দিয়ে লিংক বদলান ব্যবহার করুন।",
  "unavailable.needsSignIn":
    "অনুপলব্ধ: সার্ভার সাইন-ইন চায়, যা Downova এখনো পারে না। ফাইলটি ব্রাউজারে নামান।",
  "unavailable.pauseFirst": "অনুপলব্ধ: লিংক বদলানোর আগে ডাউনলোডটি থামান।",
  "menu.file.refreshLink": "লিংক বদলান…",
  "menu.file.openFolder": "ফোল্ডার খুলুন",
  "retry.title": "এই ডাউনলোড আবার চেষ্টা করবেন?",
  "retry.body":
    "আবার চেষ্টা করলে ডাউনলোডটি একই নাম, ফোল্ডার, সারি ও অগ্রাধিকারসহ শুরু থেকে আবার নামবে।",
  "retry.discard":
    "আগে নামানো {size} বাদ যাবে: ত্রুটিতে থেমে যাওয়া ডাউনলোডের নিরাপদে চালিয়ে যাওয়ার মতো কোনো জায়গা থাকে না।",
  "retry.checksum": "লিংকের সাথে দেওয়া প্রকাশকের SHA-256 শেষ হওয়ার আগে আবার মেলানো হবে।",
  "retry.confirm": "শুরু থেকে আবার চেষ্টা",
  "retry.keep": "যেমন আছে রাখুন",
  "retry.pending": "আবার চেষ্টা হচ্ছে…",
  "retry.failed": "আবার চেষ্টা শুরু হয়নি",
  "retry.uncertain":
    "ডাউনলোড সার্ভিস সময়মতো নিশ্চিত করেনি। আবার চেষ্টা আবার বাছুন: একই অনুরোধ যায়, তাই ডাউনলোড কখনো দুবার শুরু হয় না।",
  "refresh.title": "লিংক বদলান",
  "refresh.label": "নতুন লিংক",
  "refresh.hint":
    "একই ফাইলের নতুন লিংক, যেমন ওয়েবসাইটের ডাউনলোড পাতা থেকে। Downova একই ফাইল বলে প্রমাণ করতে পারলে তবেই অগ্রগতি রাখা হয়।",
  "refresh.submit": "নতুন লিংক ব্যবহার করুন",
  "refresh.cancel": "বাতিল",
  "refresh.pending": "নতুন লিংক যাচাই হচ্ছে…",
  "refresh.kept": "নতুন লিংকে একই ফাইল আছে। ডাউনলোড যেখানে ছিল সেখান থেকে চলবে।",
  "refresh.restarted": "নতুন লিংক ব্যবহার হচ্ছে। ডাউনলোড শুরু থেকে আবার নামবে।",
  "refresh.choiceTitle": "অগ্রগতি রাখা যাচ্ছে না",
  "refresh.reason.no_checkpoint": "চালিয়ে যাওয়ার মতো কোনো সংরক্ষিত অগ্রগতি নেই।",
  "refresh.reason.validator_missing":
    "সার্ভার এমন কোনো সংস্করণ চিহ্ন (ETag) দেয় না যা একই ফাইল বলে প্রমাণ করে।",
  "refresh.reason.validator_changed": "সার্ভার বলছে এটি ফাইলের অন্য সংস্করণ।",
  "refresh.reason.length_changed": "ফাইলের আকার আলাদা।",
  "refresh.reason.range_ignored": "সার্ভার মাঝপথ থেকে ডাউনলোড চালাতে পারে না।",
  "refresh.reason.content_differs": "আগে নামানো ডেটা নতুন লিংকের ফাইলের সাথে মেলে না।",
  "refresh.reason.different_resource":
    "এটি অন্য ঠিকানা, এবং শেষ ফাইল প্রমাণ করার মতো প্রকাশকের চেকসাম নেই।",
  "refresh.reason.unverifiable": "নতুন লিংক যাচাই করা যায়নি।",
  "refresh.restart": "নতুন লিংক দিয়ে শূন্য থেকে শুরু",
  "refresh.keepCurrent": "বর্তমান ডাউনলোড রাখুন",
  "refresh.restartNote":
    "শূন্য থেকে শুরু করলে শুধু এই ডাউনলোডের আগে নামানো {size} বাদ যাবে। বর্তমান ডাউনলোড রাখলে কিছুই বদলায় না।",
  "refresh.failed": "লিংক বদলানো হয়নি",
  "refresh.uncertain":
    "ডাউনলোড সার্ভিস সময়মতো নিশ্চিত করেনি। বোতামটি আবার বাছুন: একই অনুরোধ যায়, তাই কিছুই দুবার হয় না।",
  "openFolder.fileMissing": "ফোল্ডার খুলেছে, কিন্তু ফাইলটি আর সেখানে নেই।",
  "openFolder.failed": "ফোল্ডার খোলা যায়নি",
  "details.checksumPublisher":
    "প্রকাশকের চেকসাম মিলেছে: ফাইলটি প্রকাশকের SHA-256 অনুযায়ী হুবহু একই।",
  "details.checksumLocal":
    "SHA-256 এখানে গণনা করা। প্রকাশকের চেকসাম দেওয়া হয়নি, তাই এটি ফাইলের ছাপ রাখে, কিন্তু প্রকাশকের ফাইল বলে প্রমাণ করে না।",
  "details.checksumPending":
    "প্রকাশকের SHA-256 দেওয়া আছে: ডাউনলোড শেষ হওয়ার আগে ফাইলটিকে এর সাথে মিলতে হবে।",
  "details.sha256": "SHA-256",
  "details.checksumMismatch":
    "প্রকাশকের পাতায় SHA-256 মিলিয়ে দেখুন। সেটি ঠিক হলে পাওয়া ফাইলটি প্রকাশকের নয়: আবার নামাতে আবার চেষ্টা, অথবা প্রকাশকের লিংক দিয়ে লিংক বদলান ব্যবহার করুন।",
  "details.retry": "আবার চেষ্টা…",
  "details.refreshLink": "লিংক বদলান…",
  "details.openFolder": "ফোল্ডার খুলুন",
  "announce.retried": "{name} শুরু থেকে আবার নামছে।",
  "announce.linkKept": "{name}-এর লিংক বদলানো হয়েছে; ডাউনলোড যেখানে ছিল সেখান থেকে চলবে।",
  "announce.linkRestarted": "{name}-এর লিংক বদলানো হয়েছে; ডাউনলোড শুরু থেকে নামবে।",
  "options.browse": "ব্রাউজ…",
  "options.browseFor": "{category}-এর ফোল্ডার বাছুন",
  "options.browseFailed": "ফোল্ডার বাছার উইন্ডো খোলা যায়নি। এর বদলে ফোল্ডারের ঠিকানা লিখুন।",
  "options.browseDesktopOnly":
    "অনুপলব্ধ: শুধু Downova ডেস্কটপ অ্যাপ ফোল্ডার বাছার উইন্ডো দেখাতে পারে।",
};

export const catalogs = { en, bn } satisfies Record<string, Messages>;
export type Locale = keyof typeof catalogs;
export const LOCALES = Object.keys(catalogs) as Locale[];

/** Replaces `{name}` placeholders. Values are inserted as plain text. */
export function format(template: string, values: Record<string, string | number>): string {
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in values ? String(values[name]) : match,
  );
}
