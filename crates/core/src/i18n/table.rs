//! The table: one line per sentence, English then Italian. `{0}`, `{1}`… are filled by
//! [`super::tf`]. Add a key here and use it; the Italian is part of the line, so it cannot be
//! forgotten.

macro_rules! strings {
    ($($key:ident => $en:literal, $it:literal;)*) => {
        /// A sentence of the interface.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Key {
            $($key,)*
        }

        impl Key {
            /// Every key, for the tests.
            pub const ALL: &'static [Key] = &[$(Key::$key,)*];

            /// The English text.
            pub const fn en(self) -> &'static str {
                match self {
                    $(Key::$key => $en,)*
                }
            }

            /// The Italian text.
            pub const fn it(self) -> &'static str {
                match self {
                    $(Key::$key => $it,)*
                }
            }
        }
    };
}

/// Keys whose text is the same in both languages (a name, a unit, a key): the only ones the
/// test lets stay a copy.
#[cfg(test)]
pub(super) const SAME_IN_BOTH: &[Key] = &[
    Key::Markdown,
    Key::FileOne,
    Key::TagViewTitle,
    Key::PreRelease,
    Key::BaseIn,
    Key::StepWFile,
    Key::StepWLink,
    Key::StepWHardLink,
    Key::HelpMouse,
    Key::HOk,
    Key::HomeLabel,
    Key::AboutChannelValue,
    Key::PvPdfPages,
    Key::PolicyChange,
    Key::ScanTotals,
    Key::DocMouse,
    Key::DocNo,
    Key::BarZoxide,
    Key::PanelNav,
    Key::GitLabel,
];

strings! {
    // ---- counts
    ItemOne => "item", "elemento";
    ItemMany => "items", "elementi";
    Markdown => "Markdown", "Markdown";
    FolderOne => "folder", "cartella";
    FolderMany => "folders", "cartelle";
    FileOne => "file", "file";
    FileMany => "files", "file";
    StepOne => "step", "passaggio";
    StepMany => "steps", "passaggi";
    LinkOne => "link", "collegamento";
    LinkMany => "links", "collegamenti";

    // ---- what an operation is called (the verbs of the plan window and of the journal)
    VerbCopy => "Copy", "Copia";
    VerbMove => "Move", "Sposta";
    VerbRename => "Rename", "Rinomina";
    VerbBulkRename => "Bulk rename", "Rinomina in blocco";
    VerbNewFolder => "New folder", "Nuova cartella";
    VerbNewFile => "New file", "Nuovo file";
    VerbTrash => "Move to trash", "Sposta nel cestino";
    VerbDelete => "Delete permanently", "Elimina definitivamente";
    VerbUndo => "Undo", "Annulla";
    VerbExtract => "Extract", "Estrai";
    VerbCompress => "Compress", "Comprimi";
    VerbTags => "Change tags", "Cambia i tag";

    // What happens to one item (lower case, in the list of the plan)
    ActCopy => "copy", "copia";
    ActMove => "move", "sposta";
    ActMerge => "merge", "unisci";
    ActKeepBoth => "keep both", "tieni entrambi";
    ActReplace => "replace", "sostituisci";
    ActSkip => "skip", "salta";
    ActTrash => "trash", "cestina";
    ActDelete => "delete", "elimina";
    ActRename => "rename", "rinomina";
    ActCreate => "create", "crea";
    ActRetag => "retag", "cambia tag";

    PolicySkip => "skip existing", "salta quelli esistenti";
    PolicyKeepBoth => "keep both", "tieni entrambi";
    PolicyOverwrite => "overwrite (old goes to trash)", "sovrascrivi (il vecchio va nel cestino)";

    // ---- the titles of plans
    TitleTransfer => "{0} {1} to {2}", "{0} {1} in {2}";
    TitleTrash => "Move {0} to the trash", "Sposta {0} nel cestino";
    TitleDelete => "Permanently delete {0}", "Elimina definitivamente {0}";
    TitleRename => "Rename {0} to {1}", "Rinomina {0} in {1}";
    TitleBulkRename => "Rename {0}", "Rinomina {0}";
    TitleUndo => "Undo: {0}", "Annulla: {0}";
    TitleMkdir => "Create folder {0}", "Crea la cartella {0}";
    TitleMkfile => "Create file {0}", "Crea il file {0}";
    TitleExtract => "Extract {0}", "Estrai {0}";
    TitleExtractTo => "Extract {0} to {1}", "Estrai {0} in {1}";
    TitleExtractFrom => "Extract {0} from {1} to {2}", "Estrai {0} da {1} in {2}";
    TitleCompress => "Compress {0} into {1}", "Comprimi {0} in {1}";
    TitleTagsOn => "{0} on {1}", "{0} su {1}";
    TagAdd => "tag {0}", "aggiungi il tag {0}";
    TagRemove => "remove tag {0}", "togli il tag {0}";
    TagClear => "remove all tags", "togli tutti i tag";
    TagSet => "set tags to {0}", "imposta i tag a {0}";
    TagNone => "none", "nessuno";
    TagsNoStore => "this system keeps no tags", "questo sistema non conserva i tag";

    // ---- the steps of a plan
    StepMakeDir => "create folder {0}", "crea la cartella {0}";
    StepMakeFile => "create file {0}", "crea il file {0}";
    StepFinishDir => "finish folder {0}", "completa la cartella {0}";
    StepCopy => "copy {0} -> {1}", "copia {0} -> {1}";
    StepMoveFile => "move {0} -> {1}", "sposta {0} -> {1}";
    StepCopyLink => "copy link {0} -> {1}", "copia il collegamento {0} -> {1}";
    StepMoveLink => "move link {0} -> {1}", "sposta il collegamento {0} -> {1}";
    StepExtract => "extract {0} -> {1}", "estrai {0} -> {1}";
    StepExtractLink => "extract link {0} -> {1}", "estrai il collegamento {0} -> {1}";
    StepCompress => "compress {0} items into {1}", "comprimi {0} elementi in {1}";
    StepHardLink => "hard link {0} -> {1}", "collegamento fisico {0} -> {1}";
    StepRename => "rename {0} -> {1}", "rinomina {0} -> {1}";
    StepTrash => "trash {0}", "cestina {0}";
    StepRemoveFile => "delete {0}", "elimina {0}";
    StepRemoveDir => "remove folder {0}", "rimuovi la cartella {0}";
    StepRestore => "restore {0}", "ripristina {0}";
    StepTags => "tags of {0}: {1} -> {2}", "tag di {0}: {1} -> {2}";

    // ---- the journal: the Undo button says what it would undo
    DoneCopy => "copy of {0}", "copia di {0}";
    DoneMove => "move of {0}", "spostamento di {0}";
    DoneRename => "rename of {0}", "rinomina di {0}";
    DoneNewFolder => "new folder", "nuova cartella";
    DoneNewFile => "new file", "nuovo file";
    DoneTrash => "trash of {0}", "cestino di {0}";
    DoneDelete => "deletion of {0}", "eliminazione di {0}";
    DoneExtract => "extraction of {0}", "estrazione di {0}";
    DoneCompress => "compression of {0}", "compressione di {0}";
    DoneTags => "tag change of {0}", "modifica dei tag di {0}";
    DoneUndo => "undo", "annullamento";

    // ---- warnings of a plan ({0} is the count, {1} a detail already in brackets)
    WIrreversible => "PERMANENT: this cannot be undone", "DEFINITIVO: non si può annullare";
    WInsideItselfOne => "{0} folder is being copied or moved into itself", "{0} cartella viene copiata o spostata dentro se stessa";
    WInsideItselfMany => "{0} folders are being copied or moved into themselves", "{0} cartelle vengono copiate o spostate dentro se stesse";
    WNotWritable => "destination is not writable{0}", "la destinazione non è scrivibile{0}";
    WLowSpace => "not enough free space{0}", "spazio libero insufficiente{0}";
    WConflictOne => "{0} item already exists", "{0} elemento esiste già";
    WConflictMany => "{0} items already exist", "{0} elementi esistono già";
    WOverwriteOne => "{0} item will be replaced (the old version goes to the trash, so undo restores it)", "{0} elemento verrà sostituito (la versione vecchia va nel cestino, quindi annullando torna com'era)";
    WOverwriteMany => "{0} items will be replaced (the old versions go to the trash, so undo restores them)", "{0} elementi verranno sostituiti (le versioni vecchie vanno nel cestino, quindi annullando tornano com'erano)";
    WUnreadableOne => "{0} item cannot be read and will probably fail{1}", "{0} elemento non si può leggere e probabilmente fallirà{1}";
    WIncompleteListOne => "{0} folder could not be listed completely, so the plan may be missing items{1}", "{0} cartella non si è potuta elencare per intero, quindi il piano potrebbe non avere tutti gli elementi{1}";
    WIncompleteListMany => "{0} folders could not be listed completely, so the plan may be missing items{1}", "{0} cartelle non si sono potute elencare per intero, quindi il piano potrebbe non avere tutti gli elementi{1}";
    WUnreadableMany => "{0} items cannot be read and will probably fail{1}", "{0} elementi non si possono leggere e probabilmente falliranno{1}";
    WSpecialFileOne => "{0} special file (device, socket or pipe) will be skipped", "{0} file speciale (dispositivo, socket o pipe) verrà saltato";
    WSpecialFileMany => "{0} special files (devices, sockets or pipes) will be skipped", "{0} file speciali (dispositivi, socket o pipe) verranno saltati";
    WBrokenSymlinkOne => "{0} symlink points to nothing; it is kept as a broken link", "{0} collegamento non porta a nulla; resta un collegamento interrotto";
    WBrokenSymlinkMany => "{0} symlinks point to nothing; they are kept as broken links", "{0} collegamenti non portano a nulla; restano collegamenti interrotti";
    WSymlinkOne => "{0} symlink will be kept as a link (not followed)", "{0} collegamento resterà un collegamento (non viene seguito)";
    WSymlinkMany => "{0} symlinks will be kept as links (not followed)", "{0} collegamenti resteranno collegamenti (non vengono seguiti)";
    WCircularLinkOne => "{0} symlink forms a loop; kept as a link, not followed", "{0} collegamento forma un anello; resta un collegamento, non viene seguito";
    WCircularLinkMany => "{0} symlinks form a loop; kept as links, not followed", "{0} collegamenti formano un anello; restano collegamenti, non vengono seguiti";
    WMountLoopOne => "{0} folder loops back to a parent (mount cycle) and will be skipped", "{0} cartella torna a una cartella superiore (ciclo di mount) e verrà saltata";
    WMountLoopMany => "{0} folders loop back to a parent (mount cycle) and will be skipped", "{0} cartelle tornano a una cartella superiore (ciclo di mount) e verranno saltate";
    WCrossDevice => "different filesystem: files are copied, verified, then removed from the source", "file system diverso: i file vengono copiati, verificati e poi tolti dall'origine";
    WSameLocationOne => "{0} item is already in the destination folder", "{0} elemento è già nella cartella di destinazione";
    WSameLocationMany => "{0} items are already in the destination folder", "{0} elementi sono già nella cartella di destinazione";
    WHardLinksOne => "{0} file also has hard links outside the selection; its copy is an independent file", "{0} file ha anche collegamenti fisici fuori dalla selezione; la sua copia è un file indipendente";
    WHardLinksMany => "{0} files also have hard links outside the selection; their copies are independent files", "{0} file hanno anche collegamenti fisici fuori dalla selezione; le loro copie sono file indipendenti";
    WHardLinksKeptOne => "{0} hard link stays a hard link of the others in the copy", "{0} collegamento fisico resta collegato agli altri nella copia";
    WHardLinksKeptMany => "{0} hard links stay hard links of each other in the copy", "{0} collegamenti fisici restano collegati tra loro nella copia";
    WSparseOne => "{0} file is sparse: the holes are kept, only the data is copied", "{0} file è sparso: i vuoti restano, si copiano solo i dati";
    WSparseMany => "{0} files are sparse: the holes are kept, only the data is copied", "{0} file sono sparsi: i vuoti restano, si copiano solo i dati";
    WXattrsLostOne => "{0} item carries extended attributes that the destination does not support; it is copied without them", "{0} elemento ha attributi estesi che la destinazione non supporta; viene copiato senza";
    WXattrsLostMany => "{0} items carry extended attributes that the destination does not support; they are copied without them", "{0} elementi hanno attributi estesi che la destinazione non supporta; vengono copiati senza";
    WAclLostOne => "{0} item carries ACLs that the destination does not support; it is copied without them", "{0} elemento ha ACL che la destinazione non supporta; viene copiato senza";
    WAclLostMany => "{0} items carry ACLs that the destination does not support; they are copied without them", "{0} elementi hanno ACL che la destinazione non supporta; vengono copiati senza";
    WOwnerNotKeptOne => "{0} item belongs to someone else; the copy will belong to you (only the owner or root can keep owner and group)", "{0} elemento appartiene a qualcun altro; la copia sarà tua (solo il proprietario o root può mantenere proprietario e gruppo)";
    WOwnerNotKeptMany => "{0} items belong to someone else; the copies will belong to you (only the owner or root can keep owner and group)", "{0} elementi appartengono a qualcun altro; le copie saranno tue (solo il proprietario o root può mantenere proprietario e gruppo)";
    WAttributesNotKept => "extended attributes and ACLs are not preserved on this platform yet (tags are, where the system has them)", "gli attributi estesi e le ACL non vengono ancora conservati su questa piattaforma (i tag sì, dove il sistema li ha)";
    WCannotTagOne => "{0} item cannot carry tags here and will be left as it is{1}", "{0} elemento qui non può avere tag e resta com'è{1}";
    WCannotTagMany => "{0} items cannot carry tags here and will be left as they are{1}", "{0} elementi qui non possono avere tag e restano come sono{1}";
    WInvalidName => "invalid name{0}", "nome non valido{0}";
    WMissingOne => "{0} item no longer exists", "{0} elemento non esiste più";
    WMissingMany => "{0} items no longer exist", "{0} elementi non esistono più";
    WMergeOne => "{0} folder will be merged with the existing one", "{0} cartella verrà unita a quella esistente";
    WMergeMany => "{0} folders will be merged with existing folders", "{0} cartelle verranno unite a quelle esistenti";
    WModifiedSinceOne => "{0} item changed since the operation and will be left untouched", "{0} elemento è cambiato dopo l'operazione e non verrà toccato";
    WModifiedSinceMany => "{0} items changed since the operation and will be left untouched", "{0} elementi sono cambiati dopo l'operazione e non verranno toccati";
    WCannotUndoOne => "{0} step cannot be undone{1}", "{0} passaggio non si può annullare{1}";
    WCannotUndoMany => "{0} steps cannot be undone{1}", "{0} passaggi non si possono annullare{1}";
    WDeviceWrite => "phone or camera: each file is written under its final name and only reaches the device when it is closed, so a big file can wait at 100% for a while; a copy that fails or is cancelled removes the file", "telefono o fotocamera: ogni file è scritto con il nome definitivo e arriva sul dispositivo solo alla chiusura, quindi un file grande può restare a lungo al 100%; una copia che fallisce o è annullata toglie il file";
    WDeviceAttrs => "phone or camera: permissions, owner and extended attributes are not kept, and dates only if the device accepts them", "telefono o fotocamera: permessi, proprietario e attributi estesi non si conservano, e le date solo se il dispositivo le accetta";
    WDeviceLinkOne => "{0} link cannot be stored on a phone or camera and is skipped{1}", "{0} collegamento non si può salvare su un telefono o una fotocamera e viene saltato{1}";
    WDeviceLinkMany => "{0} links cannot be stored on a phone or camera and are skipped{1}", "{0} collegamenti non si possono salvare su un telefono o una fotocamera e vengono saltati{1}";
    WDeviceHardLinkOne => "{0} file is a hard link: on a phone or camera it is copied as a separate file{1}", "{0} file è un collegamento fisico: su un telefono o una fotocamera si copia come file separato{1}";
    WDeviceHardLinkMany => "{0} files are hard links: on a phone or camera they are copied as separate files{1}", "{0} file sono collegamenti fisici: su un telefono o una fotocamera si copiano come file separati{1}";
    WDeviceUndo => "phone or camera: undo is not guaranteed, because the device cannot promise that what is there stays as rada left it", "telefono o fotocamera: l'annullamento non è garantito, perché il dispositivo non può promettere che ciò che c'è resti come l'ha lasciato rada";
    WDeviceNoTrash => "a phone or camera has no trash: each item is copied to your trash first (slow for big files), checked, and then removed from the device", "un telefono o una fotocamera non ha il cestino: ogni elemento viene prima copiato nel tuo cestino (lento per i file grandi), controllato e poi tolto dal dispositivo";
    WDeviceArchive => "an archive cannot be written straight onto a phone or camera: make it in a folder of this computer, then copy it", "un archivio non si può scrivere direttamente su un telefono o una fotocamera: crealo in una cartella di questo computer e poi copialo";
    WDeviceRename => "phone or camera: renaming and moving are not atomic and some devices refuse them; if it fails, the item keeps its name", "telefono o fotocamera: rinominare e spostare non sono operazioni atomiche e alcuni dispositivi le rifiutano; se fallisce, l'elemento resta com'è";
    WUnsafePathOne => "{0} item is NOT extracted: its path would write outside the destination{1}", "{0} elemento NON viene estratto: il suo percorso scriverebbe fuori dalla destinazione{1}";
    WUnsafePathMany => "{0} items are NOT extracted: their paths would write outside the destination{1}", "{0} elementi NON vengono estratti: i loro percorsi scriverebbero fuori dalla destinazione{1}";
    WLinkOutsideOne => "{0} link points outside the extracted folder; it is created as a link and never followed", "{0} collegamento punta fuori dalla cartella estratta; viene creato come collegamento e non viene mai seguito";
    WLinkOutsideMany => "{0} links point outside the extracted folder; they are created as links and never followed", "{0} collegamenti puntano fuori dalla cartella estratta; vengono creati come collegamenti e non vengono mai seguiti";
    WArchiveBomb => "this archive may be a decompression bomb{0}; type yes to extract it anyway", "questo archivio potrebbe essere una bomba di decompressione{0}; scrivi sì per estrarlo comunque";
    WArchiveDamaged => "the archive is damaged or incomplete{0}: only what could be read is shown", "l'archivio è danneggiato o incompleto{0}: si vede solo ciò che si è potuto leggere";
    WEncryptedOne => "{0} item is protected by a password, which rada cannot use yet", "{0} elemento è protetto da una password, che rada non sa ancora usare";
    WEncryptedMany => "{0} items are protected by a password, which rada cannot use yet", "{0} elementi sono protetti da una password, che rada non sa ancora usare";
    WZipLinksOne => "{0} symlink is stored as a link; programs that do not know ZIP links (Windows Explorer, for one) extract it as a small text file", "{0} collegamento è salvato come collegamento; i programmi che non conoscono i collegamenti ZIP (Esplora file di Windows, per dirne uno) lo estraggono come un piccolo file di testo";
    WZipLinksMany => "{0} symlinks are stored as links; programs that do not know ZIP links (Windows Explorer, for one) extract them as small text files", "{0} collegamenti sono salvati come collegamenti; i programmi che non conoscono i collegamenti ZIP (Esplora file di Windows, per dirne uno) li estraggono come piccoli file di testo";
    WNamesChangedOne => "{0} name cannot be stored exactly in this format and will be changed{1}", "{0} nome non si può salvare esattamente in questo formato e verrà cambiato{1}";
    WNamesChangedMany => "{0} names cannot be stored exactly in this format and will be changed{1}", "{0} nomi non si possono salvare esattamente in questo formato e verranno cambiati{1}";
    WSetuidDroppedOne => "setuid/setgid permission bits are dropped from {0} file", "i permessi setuid/setgid vengono tolti da {0} file";
    WSetuidDroppedMany => "setuid/setgid permission bits are dropped from {0} files", "i permessi setuid/setgid vengono tolti da {0} file";
    WLinkTargetMissingOne => "{0} hard link points to a file that is not part of the selection and is skipped", "{0} collegamento fisico punta a un file che non fa parte della selezione e viene saltato";
    WLinkTargetMissingMany => "{0} hard links point to a file that is not part of the selection and are skipped", "{0} collegamenti fisici puntano a un file che non fa parte della selezione e vengono saltati";
    WOther => "see details", "vedi i dettagli";
    // ---- dates and times
    JustNow => "just now", "adesso";
    MinAgoOne => "1 min ago", "1 min fa";
    MinAgo => "{0} min ago", "{0} min fa";
    HoursAgo => "{0} h ago", "{0} h fa";
    YesterdayLower => "yesterday", "ieri";
    Yesterday => "Yesterday", "Ieri";
    DaysAgo => "{0} days ago", "{0} giorni fa";
    WeeksAgo => "{0} weeks ago", "{0} settimane fa";
    Today => "Today {0}", "Oggi {0}";
    TodayComma => "Today, {0}", "Oggi, {0}";
    YesterdayComma => "Yesterday, {0}", "Ieri, {0}";
    MonJan => "Jan", "gen";
    MonFeb => "Feb", "feb";
    MonMar => "Mar", "mar";
    MonApr => "Apr", "apr";
    MonMay => "May", "mag";
    MonJun => "Jun", "giu";
    MonJul => "Jul", "lug";
    MonAug => "Aug", "ago";
    MonSep => "Sep", "set";
    MonOct => "Oct", "ott";
    MonNov => "Nov", "nov";
    MonDec => "Dec", "dic";

    // ---- the names of keys, as they are written on a key cap
    KeySpace => "Space", "Spazio";
    KeyEnter => "Enter", "Invio";
    KeyDel => "Del", "Canc";

    // ---- the groups of the help
    GroupMove => "Moving around", "Spostarsi";
    GroupSelect => "Selecting", "Selezionare";
    GroupFiles => "Working with files", "Lavorare con i file";
    GroupView => "Looking", "Visualizzare";
    GroupTabs => "Tabs", "Schede";
    GroupProgram => "The program", "Il programma";

    // ---- what each action is called (help, menus, the palette)
    LUp => "Move up", "Su";
    LDown => "Move down", "Giù";
    LParent => "Parent folder", "Cartella superiore";
    LOpen => "Open", "Apri";
    LFirst => "First item", "Primo elemento";
    LLast => "Last item", "Ultimo elemento";
    LPageUp => "Page up", "Pagina su";
    LPageDown => "Page down", "Pagina giù";
    LHalfPageUp => "Half page up", "Mezza pagina su";
    LHalfPageDown => "Half page down", "Mezza pagina giù";
    LToggleMark => "Select / unselect item", "Seleziona / deseleziona l'elemento";
    LSelectAll => "Select all", "Seleziona tutto";
    LSelectUp => "Extend selection up", "Estendi la selezione in su";
    LSelectDown => "Extend selection down", "Estendi la selezione in giù";
    LSelectToFirst => "Extend selection to start", "Estendi la selezione all'inizio";
    LSelectToLast => "Extend selection to end", "Estendi la selezione alla fine";
    LClearSelection => "Clear selection / cancel", "Deseleziona / interrompi";
    LCopy => "Copy", "Copia";
    LCut => "Cut", "Taglia";
    LPaste => "Paste", "Incolla";
    LTrash => "Move to trash", "Sposta nel cestino";
    LDeletePermanently => "Delete permanently", "Elimina definitivamente";
    LRename => "Rename", "Rinomina";
    LBulkRename => "Rename many", "Rinomina più elementi";
    LNewFolder => "New folder", "Nuova cartella";
    LExtractHere => "Extract here", "Estrai qui";
    LExtractToFolder => "Extract to a folder…", "Estrai in una cartella…";
    LCompress => "Compress…", "Comprimi…";
    LUndo => "Undo", "Annulla";
    LRedo => "Redo", "Ripeti";
    LHistory => "Operation history", "Cronologia delle operazioni";
    LFilter => "Filter this folder", "Filtra questa cartella";
    LPalette => "Jump to…", "Vai a…";
    LSort => "Change sort order", "Cambia l'ordinamento";
    LSortReverse => "Reverse sort", "Inverti l'ordinamento";
    LToggleHidden => "Show / hide hidden files", "Mostra / nascondi i file nascosti";
    LToggleHex => "Hex dump of a binary file", "Dump esadecimale di un file binario";
    LPreviewDown => "Scroll preview down", "Scorri l'anteprima in giù";
    LPreviewUp => "Scroll preview up", "Scorri l'anteprima in su";
    LBookmark => "Pin / unpin this folder", "Fissa / stacca questa cartella";
    LGoHome => "Go to home", "Vai alla Home";
    LBack => "Back to the previous folder", "Torna alla cartella precedente";
    LForward => "Forward to the next folder", "Avanti alla cartella successiva";
    LSwitchPane => "Move between the list and the sidebar", "Passa dall'elenco alla barra laterale";
    LToggleSidebar => "Show / hide the sidebar", "Mostra / nascondi la barra laterale";
    LHelp => "Help", "Aiuto";
    LQuit => "Quit", "Esci";
    LNewFile => "New empty file", "Nuovo file vuoto";
    LRefresh => "Refresh the folder", "Aggiorna la cartella";
    LFocusAddress => "Edit the address", "Modifica l'indirizzo";
    LToggleDetails => "Show / hide the details pane", "Mostra / nascondi il pannello dei dettagli";
    LViewDetails => "View as details", "Vista a dettagli";
    LViewIcons => "View as icons", "Vista a icone";
    LToggleView => "Switch the view: Details, Icons, Columns", "Cambia vista: Dettagli, Icone, Colonne";
    LProperties => "Properties", "Proprietà";
    LOpenWith => "Open with…", "Apri con…";
    LCopyPath => "Copy the path", "Copia il percorso";
    LOpenTerminal => "Open a terminal here", "Apri un terminale qui";
    LContextMenu => "Open the context menu", "Apri il menu contestuale";
    LNewTab => "New tab", "Nuova scheda";
    LCloseTab => "Close tab", "Chiudi scheda";
    LNextTab => "Next tab", "Scheda successiva";
    LPrevTab => "Previous tab", "Scheda precedente";
    LUpdate => "Update rada", "Aggiorna rada";
    LQuickLook => "Quick Look", "Anteprima rapida";
    LShowPackage => "Show package contents", "Mostra il contenuto del pacchetto";
    LShowInFolder => "Show in folder", "Mostra nella cartella";
    ResultOne => "result", "risultato";
    ResultMany => "results", "risultati";

    // ---- the details pane
    PropDimensions => "Dimensions", "Dimensioni";
    PropPages => "Pages", "Pagine";
    PropTitle => "Title", "Titolo";
    PropAuthor => "Author", "Autore";
    PropContains => "Contains", "Contiene";
    PropSize => "Size", "Peso";
    PropTags => "Tags", "Tag";
    PropModified => "Modified", "Modificato";
    PropCreated => "Created", "Creato";
    PropLocation => "Location", "Posizione";
    PropPermissions => "Permissions", "Permessi";
    PropAttributes => "Attributes", "Attributi";
    PageOne => "1 page", "1 pagina";
    PageMany => "{0} pages", "{0} pagine";
    PagesUnknown => "unknown", "sconosciute";
    PropSizeBytes => "{0} ({1} bytes)", "{0} ({1} byte)";
    FlagReadOnly => "read-only", "sola lettura";
    FlagHidden => "hidden", "nascosto";
    FlagSystem => "system", "di sistema";
    FlagNormal => "normal", "normale";
    SelItemsOne => "1 item selected", "1 elemento selezionato";
    SelItemsMany => "{0} items selected", "{0} elementi selezionati";
    InFiles => "{0} in files", "{0} nei file";
    SelectToSee => "Select a file to see its details", "Seleziona un file per vederne i dettagli";
    BtnOpen => "Open", "Apri";
    BtnPreview => "Preview", "Anteprima";
    BtnOpenWith => "Open with…", "Apri con…";
    BtnQuickLook => "Quick Look", "Anteprima rapida";

    // ---- the properties window
    PropItems => "Items", "Elementi";
    PropName => "Name", "Nome";
    PropType => "Type", "Tipo";
    PropLinkTo => "Link to", "Collegamento a";
    PropProblem => "Problem", "Problema";
    PropInside => "Inside", "Dentro";
    PropItemsValue => "{0} ({1} files, {2})", "{0} ({1} file, {2})";
    PropSizeNoFolders => "{0} ({1} bytes), folders not counted", "{0} ({1} byte), cartelle escluse";
    PropInArchive => "an archive (read-only)", "un archivio (sola lettura)";
    FlagExecutable => "executable", "eseguibile";
    TagViewTitle => "Tag: {0}", "Tag: {0}";
    PropsSelectedOne => "1 selected", "1 selezionato";
    PropsSelectedMany => "{0} selected", "{0} selezionati";

    // ---- windows about the system, disks and updates
    ChipNoAccess => "No access", "Accesso negato";
    MacAsksOnce => "macOS asks you once, for each program, before it lets it into the Desktop, Documents, Downloads, iCloud Drive and external or network disks.", "macOS chiede il permesso una volta, per ogni programma, prima di lasciarlo entrare in Scrivania, Documenti, Download, iCloud Drive e nei dischi esterni o di rete.";
    BtnOpenFilesSettings => "Open Files & Folders settings", "Apri le impostazioni File e cartelle";
    BtnFullDisk => "Full Disk Access", "Accesso completo al disco";
    BtnClose => "Close", "Chiudi";
    BtnCancel => "Cancel", "Chiudi";
    BtnHide => "Hide", "Nascondi";
    BtnTryAgain => "Try again", "Riprova";
    BtnEject => "Eject", "Espelli";
    EjectFinishFirst => "Everything in progress on this disk is finished first. After this you can unplug it.", "Prima si conclude tutto ciò che è in corso su questo disco. Poi lo puoi scollegare.";
    Ejecting => "Ejecting…", "Espulsione in corso…";
    NotEjected => "It was not ejected: {0}", "Non è stato espulso: {0}";
    ChipIcons => "Icons", "Icone";
    FontCannotDraw => "Your terminal cannot draw rada's icons yet", "Il terminale non sa ancora disegnare le icone di rada";
    FontOffer => "File-type icons need a Nerd Font, and this terminal is not set to use one. rada can install {0} for you: about {1} MB, taken from the Nerd Fonts project at a fixed release and checked against its checksums, for your account only (no administrator rights). Afterwards you choose it in your terminal's font setting. This is asked only once.", "Le icone dei tipi di file richiedono un Nerd Font, e questo terminale non ne usa uno. rada può installare {0} per te: circa {1} MB, presi dal progetto Nerd Fonts a una versione fissa e verificati con i loro checksum, solo per il tuo account (senza diritti di amministratore). Poi lo scegli nelle impostazioni del carattere del terminale. Lo chiediamo una volta sola.";
    BtnInstallFont => "Install the font", "Installa il font";
    BtnNotNow => "Not now", "Non ora";
    FontWorking => "Downloading and installing…", "Scaricamento e installazione…";
    ChipUpdate => "Update", "Aggiornamento";
    UpdateAvailable => "rada {0} is available{1}", "rada {0} è disponibile{1}";
    PreRelease => " (pre-release)", " (pre-release)";
    WhatIsNew => "What is new", "Novità";
    NoReleaseNotes => "(no release notes)", "(nessuna nota di rilascio)";
    UpdateBinary => "rada will download the program for this system, check the signature of the release's SHA256SUMS and the program against it, and replace this one. The old one is kept: `rada update --rollback` brings it back.", "rada scaricherà il programma per questo sistema, controllerà la firma degli SHA256SUMS della versione e il programma con essi, e sostituirà questo. Quello vecchio viene conservato: `rada update --rollback` lo riporta.";
    UpdateSource => "rada will update the git checkout in {0} (it must be on main with nothing uncommitted), build it, put the new program in place of this one and clean the build folder. This takes a few minutes; you can keep working.", "rada aggiornerà la copia git in {0} (deve essere su main e senza modifiche non salvate), la compilerà, metterà il nuovo programma al posto di questo e pulirà la cartella di compilazione. Ci vuole qualche minuto; puoi continuare a lavorare.";
    BtnUpdateNow => "Update now", "Aggiorna ora";
    BtnLater => "Later", "Più tardi";
    UpdateFailed => "The update did not work", "L'aggiornamento non è riuscito";
    UpdateUntouched => "The program that was running is untouched.", "Il programma in uso non è stato toccato.";
    LogLine => "Log: {0}", "Registro: {0}";
    BtnOpenLog => "Open the log", "Apri il registro";

    // ---- windows in front of the interface
    HintClose => "close", "chiudi";
    HintScroll => "scroll", "scorri";
    HintCancel => "cancel", "interrompi";
    HintMove => "move", "sposta";
    HintGo => "go", "vai";
    ChipProperties => "Properties", "Proprietà";
    ScanTotals => "{0} · {1} · {2}", "{0} · {1} · {2}";
    ChipError => "Error", "Errore";
    WentWrong => "Something went wrong", "Qualcosa è andato storto";
    AttemptN => "attempt {0}", "tentativo {0}";
    BtnSkip => "Skip", "Salta";
    BtnSkipAll => "Skip all", "Salta tutto";
    BtnRetry => "Retry", "Riprova";
    BtnAbort => "Abort", "Interrompi";
    AbortKeeps => "Abort keeps what is already done; u undoes it.", "Interrompi conserva ciò che è già fatto; u lo annulla.";
    JournalFailTitle => "The journal cannot be written", "Il registro non si può scrivere";
    JournalFailNote => "The next steps could not be undone. Stopping keeps what is already done, and u undoes it.", "I passaggi successivi non si potrebbero annullare. Interrompendo si conserva ciò che è già fatto, e u lo annulla.";
    BtnContinueNoUndo => "Continue without being able to undo the rest", "Continua senza poter annullare il resto";
    NJournalStopped => "Stopped because the journal could not be written ({0}). Undo covers what came before {1}.", "Fermata perché il registro non si è potuto scrivere ({0}). L'annullamento copre ciò che viene prima di {1}.";
    NJournalOff => "The journal could not be written ({0}). From {1} on this operation cannot be undone.", "Il registro non si è potuto scrivere ({0}). Da {1} in poi questa operazione non si può annullare.";
    HistPartlyUndoable => "undoable in part", "annullabile in parte";
    ChipDone => "Done", "Fatto";
    BtnUndoDone => "Undo what was done", "Annulla ciò che è stato fatto";
    ChipRunning => "Running", "In corso";
    OpInProgress => "An operation is in progress", "Un'operazione è in corso";
    QuitWarn1 => "Quitting cancels it. Steps already finished stay in the", "Uscendo la interrompi. I passaggi già conclusi restano nel";
    QuitWarn2 => "journal and can be undone.", "registro e si possono annullare.";
    BtnQuitAnyway => "Quit anyway", "Esci comunque";
    BtnStay => "Stay", "Resta";
    VCopy => "Copy", "Copia";
    VMove => "Move", "Sposta";
    VRename => "Rename", "Rinomina";
    VNewFolder => "New folder", "Nuova cartella";
    VNewFile => "New file", "Nuovo file";
    VTrash => "Trash", "Cestina";
    VDelete => "Delete", "Elimina";
    VUndo => "Undo", "Annulla";
    VExtract => "Extract", "Estrai";
    VCompress => "Compress", "Comprimi";
    VTags => "Tags", "Tag";
    BaseFrom => "From", "Da";
    BaseTo => "To", "A";
    BaseIn => "In", "In";
    StatFreed => "freed", "liberati";
    StatToTrash => "to the trash", "nel cestino";
    StatInTotal => "in total", "in totale";
    WillBeWritten => "{0} will actually be written", "{0} verranno effettivamente scritti";
    PolicyAsk => "If a name already exists", "Se il nome esiste già";
    PolSkip => "skip", "salta";
    PolKeep => "keep both", "tieni entrambi";
    PolOver => "overwrite", "sovrascrivi";
    PolicyChange => "← →", "← →";
    RenameCannotReplace => "only a file can replace a file, and a folder an empty folder", "solo un file può sostituire un file, e una cartella una cartella vuota";
    PlanQDelete => "Delete {0} permanently?", "Eliminare definitivamente {0}?";
    PlanQTrash => "Move {0} to the trash?", "Spostare {0} nel cestino?";
    UndoQ => "Undo {0}?", "Annullare {0}?";
    RedoQ => "Repeat {0}?", "Ripetere {0}?";
    BtnUndoOp => "Undo {0}", "Annulla {0}";
    BtnRedoOp => "Repeat {0}", "Ripeti {0}";
    OpOfItems => "{0} of {1}", "{0} di {1}";
    OpNounCopy => "the copy", "la copia";
    OpNounMove => "the move", "lo spostamento";
    OpNounTrash => "moving to the trash", "lo spostamento nel cestino";
    OpNounRename => "the rename", "la rinomina";
    OpNounExtract => "the extraction", "l'estrazione";
    OpNounCompress => "the compression", "la compressione";
    OpNounMkdir => "the creation of the folder", "la creazione della cartella";
    OpNounMkfile => "the creation of the file", "la creazione del file";
    OpNounTags => "the tag change", "la modifica dei tag";
    OpNounDelete => "the deletion", "l'eliminazione";
    BadgeRemoved => "will be removed", "verrà tolto";
    BadgeBack => "goes back", "torna al suo posto";
    BadgeTagsBack => "tags restored", "tag ripristinati";
    BadgeNew => "new", "nuovo";
    BadgeExists => "already there", "esiste già";
    HintTabButtons => "Tab buttons", "Tab pulsanti";
    NoteUndoCopy => "You can undo this with {0}: the new copies will be removed.", "Si potrà annullare con {0}: le copie nuove verranno tolte.";
    NoteUndoMove => "You can undo this with {0}: the items will go back where they were.", "Si potrà annullare con {0}: gli elementi torneranno dove erano.";
    NoteUndoTrash => "You can undo this with {0}: the items will come back from the trash.", "Si potrà annullare con {0}: gli elementi usciranno dal cestino.";
    NoteUndoExtract => "You can undo this with {0}: what is extracted will be removed.", "Si potrà annullare con {0}: ciò che viene estratto verrà tolto.";
    NoteUndoCompress => "You can undo this with {0}: the archive will be removed.", "Si potrà annullare con {0}: l'archivio verrà tolto.";
    NoteUndoRename => "You can undo this with {0}: the names will go back.", "Si potrà annullare con {0}: i nomi torneranno quelli di prima.";
    NoteUndoTags => "You can undo this with {0}: the tags will go back.", "Si potrà annullare con {0}: i tag torneranno quelli di prima.";
    NoteUndoNew => "You can undo this with {0}: what is made will be removed.", "Si potrà annullare con {0}: ciò che viene creato verrà tolto.";
    NoteNoUndoTrash => "Not in the trash: the files are removed for good.", "Non finiscono nel cestino: i file vengono tolti per sempre.";
    PolicyTrashNote => "replaced items go to the trash first, so undo brings them back", "gli elementi sostituiti vanno prima nel cestino, così annullando tornano";
    PlanRenames => "Renames", "Rinomine";
    PlanWhat => "What will happen", "Cosa succederà";
    PlanTabSteps => "d every step", "d ogni passaggio";
    PlanTabSummary => "d summary", "d riepilogo";
    PlanWhatN => "What will happen ({0})", "Cosa succederà ({0})";
    ActExtractItem => "extract", "estrai";
    ActAddItem => "add", "aggiungi";
    AsName => "as {0}", "come {0}";
    AndNMore => "… and {0} more", "… e altri {0}";
    MoreStepsNotListed => "… more steps not listed", "… altri passaggi non elencati";
    CannotBeUndone => "This cannot be undone: the files do not go to the trash.", "Non si può annullare: i file non passano dal cestino.";
    ArchiveMayBeBomb => "This archive may be a bomb.", "Questo archivio potrebbe essere una bomba.";
    TypePrefix => "Type ", "Scrivi ";
    ConfirmWord => "yes", "sì";
    ToConfirm => " to confirm: ", " per confermare: ";
    Replanning => "re-planning…", "nuovo piano in corso…";
    BlockedNothing => "Blocked: nothing will be done", "Bloccato: non verrà fatto nulla";
    BtnRun => "Run", "Esegui";
    ArrowsScroll => "↑↓ scroll", "↑↓ scorri";
    StepWFolder => "folder", "cartella";
    StepWFile => "file", "file";
    StepWMove => "move", "sposta";
    StepWCopy => "copy", "copia";
    StepWLink => "link", "link";
    StepWHardLink => "hlink", "hlink";
    StepWExtract => "extract", "estrai";
    StepWArchive => "archive", "archivio";
    StepWRename => "rename", "rinomina";
    StepWTrash => "trash", "cestina";
    StepWDelete => "delete", "elimina";
    StepWRemove => "remove", "rimuovi";
    StepWRestore => "restore", "ripristina";
    StepWTags => "tags", "tag";
    PaletteHint => "Jump to a folder, bookmark or disk…", "Vai a una cartella, un preferito o un disco…";
    PaletteNone => "No matches. Type a path starting with / or ~ to go anywhere.", "Nessun risultato. Scrivi un percorso che inizia con / o ~ per andare ovunque.";
    ChipOpenWith => "Open with", "Apri con";
    OpenFileWith => "Open {0} with", "Apri {0} con";
    AlwaysForExt => "Always use for .{0} files", "Usa sempre per i file .{0}";
    AlwaysForKind => "Always use for this kind of file", "Usa sempre per questo tipo di file";
    TagsOf => "Tags of {0}", "Tag di {0}";
    OpenWithHint => "Type to find a program…", "Scrivi per cercare un programma…";
    OpenWithLooking => "Looking for the programs that open it…", "Cerco i programmi che lo aprono…";
    OpenWithNone => "No installed program says it opens this kind of file.", "Nessun programma installato dichiara di aprire questo tipo di file.";
    OpenWithNoMatch => "No program matches what you typed.", "Nessun programma corrisponde a ciò che hai scritto.";
    TagAlways => "always", "sempre";
    TagDefault => "default", "predefinito";
    TagInTerminal => "in a terminal", "in un terminale";
    OtherProgram => "Other program…", "Altro programma…";

    // ---- input windows, history and help
    InNewName => "New name", "Nuovo nome";
    InName => "Name", "Nome";
    InNewFileHint => "Name (an empty file is created)", "Nome (viene creato un file vuoto)";
    ChipNewTag => "New tag", "Nuovo tag";
    InNewTagHint => "Name of the tag (the selected items get it)", "Nome del tag (lo ricevono gli elementi selezionati)";
    InOpenWithHint => "Program, with options if you like — for example: code, vlc --fullscreen", "Programma, con le opzioni se vuoi — per esempio: code, vlc --fullscreen";
    InExtractHint => "Into a folder named (the archive's folder if the name is unchanged and it has one at its top)", "In una cartella chiamata (quella dell'archivio se il nome non cambia e ne ha una in cima)";
    InArchiveHint => "Archive name — Tab changes the format: {0}", "Nome dell'archivio — Tab cambia il formato: {0}";
    InBulkHint => "Pattern:  {name} {ext} {n} {n:3} {parent} {name:lower}   or   s/find/replace/", "Modello:  {name} {ext} {n} {n:3} {parent} {name:lower}   oppure   s/cerca/sostituisci/";
    BtnPlan => "Plan", "Piano";
    ChipHistory => "History", "Cronologia";
    ReadingJournal => "{0} reading the journal…", "{0} leggo il registro…";
    NoOpsYet => "No operations yet.", "Ancora nessuna operazione.";
    HistUndone => "undone", "annullata";
    HistPartly => "partly undone ({0} left)", "annullata in parte (ne restano {0})";
    HistPermanent => "permanent", "definitiva";
    HistNothing => "nothing to undo", "niente da annullare";
    HistCanUndo => "can undo", "annullabile";
    BtnUndoSelected => "Undo selected", "Annulla la selezionata";
    HelpClassic => "Classic", "Classica";
    HelpMouse => "Mouse", "Mouse";
    HelpAlso => "Also", "Inoltre";
    HelpMouse1 => "Click a name to put the cursor there · the box on the left (or Space) selects · double-click opens.", "Clicca un nome per mettere il cursore lì · la casella a sinistra (o Spazio) seleziona · il doppio clic apre.";
    HelpMouse2 => "Ctrl+click adds one item · Shift+click selects a range · right-click opens a menu · wheel scrolls.", "Ctrl+clic aggiunge un elemento · Maiusc+clic seleziona un intervallo · il clic destro apre un menu · la rotella scorre.";
    HelpMouse3 => "Every button of the bars is clickable and does what its key does; middle-click closes a tab.", "Ogni pulsante delle barre si può cliccare e fa ciò che fa il suo tasto; il clic centrale chiude una scheda.";
    HelpMouse4 => "Click the path (outside its words) to type an address; click a column title to sort by it.", "Clicca il percorso (fuori dalle parole) per scrivere un indirizzo; clicca il titolo di una colonna per ordinare.";
    HelpMouse5 => "Shift+drag selects text in the terminal (the mouse belongs to rada while it runs).", "Maiusc+trascina seleziona il testo nel terminale (il mouse è di rada finché è in esecuzione).";
    HelpMouseOff => "The mouse is off (mouse = false in the configuration).", "Il mouse è disattivato (mouse = false nella configurazione).";
    HelpAlso1 => "Alt+1 … Alt+9 jump to a tab · Alt+P shows or hides the details pane.", "Alt+1 … Alt+9 vanno a una scheda · Alt+P mostra o nasconde il pannello dei dettagli.";
    HelpAlso2 => "Ctrl+1 / Ctrl+2 / Ctrl+3 (or v) switch between Details, Icons and Columns; in Icons the arrows and h j k l move across the grid; in Columns Left goes to the folder above and Right into the folder.", "Ctrl+1 / Ctrl+2 / Ctrl+3 (o v) passano da Dettagli a Icone e Colonne; nelle Icone le frecce e h j k l si muovono nella griglia; nelle Colonne ← va alla cartella superiore e → dentro la cartella.";
    HelpAlso3 => "Backspace goes to the parent folder with the vim keys on, and back with the classic keys alone.", "Backspace va alla cartella superiore con i tasti vim attivi, e indietro con i soli tasti classici.";
    HelpAlso4 => "Shift+Del deletes for good: the plan says so, and it cannot be undone.", "Maiusc+Canc elimina per sempre: il piano lo dice, e non si può annullare.";
    HelpAlso5 => "Ctrl+L also draws the whole screen again from nothing: use it if anything has spoiled it.", "Ctrl+L ridisegna anche tutto lo schermo da zero: usalo se qualcosa lo ha sporcato.";
    HelpEnd1 => "Enter on an archive opens it like a folder, read-only: copy items out (Ctrl+C), paste in a folder.", "Invio su un archivio lo apre come una cartella, in sola lettura: copia fuori gli elementi (Ctrl+C), incolla in una cartella.";
    HelpEnd2 => "Extract here / to a folder and Compress… need no archive tool installed; RAR needs 7z or unrar.", "Estrai qui / in una cartella e Comprimi… non richiedono programmi per archivi; per i RAR servono 7z o unrar.";
    HelpEnd3 => "Ctrl+C copies; it never quits. Quit with {0}.", "Ctrl+C copia; non chiude mai il programma. Esci con {0}.";
    HelpEnd4 => "Every operation shows a plan first. Every finished operation can be undone.", "Ogni operazione mostra prima un piano. Ogni operazione conclusa si può annullare.";
    HelpEnd5 => "Rebind keys in the [keys] section of the configuration file.", "Cambia i tasti nella sezione [keys] del file di configurazione.";
    HelpEnd6 => "To have the shell go to the folder you were in when you quit, run `rada setup shell` once, then open a new terminal and start rada with `v`.", "Per far andare la shell nella cartella in cui eri quando esci, esegui una volta `rada setup shell`, poi apri un nuovo terminale e avvia rada con `v`.";
    ChipKeys => "Keys", "Tasti";
    HelpScrollClose => "↑↓ scroll · Esc close", "↑↓ scorri · Esc chiudi";
    HelpClose => "Esc close", "Esc chiudi";

    // ---- open with
    AlwaysUseFor => "{0} Always use {1} for {2}", "{0} Usa sempre {1} per {2}";
    AlwaysUseProgram => "{0} Always use the program for {1}", "{0} Usa sempre il programma per {1}";
    ThisKindOfFile => "this kind of file", "questo tipo di file";
    OnlyInRada => "Only in rada: the system's own choice is not changed.", "Solo in rada: la scelta del sistema non cambia.";
    HintOpen => "open", "apri";

    // ---- palette tags
    PalGoTo => "go to", "vai a";
    PalBookmark => "bookmark", "preferito";
    PalRecent => "recent", "recente";
    PalPlace => "place", "luogo";
    PalDisk => "disk", "disco";
    PalParent => "parent", "superiore";
    PalFolder => "folder", "cartella";

    // ---- menus
    ColName => "Name", "Nome";
    ColModified => "Date modified", "Modificato";
    ColUsed => "Last used", "Usato";
    ColType => "Type", "Tipo";
    ColSize => "Size", "Dimensione";
    ColTags => "Tags", "Tag";
    SortModified => "Date modified", "Data di modifica";
    Ascending => "Ascending", "Crescente";
    Descending => "Descending", "Decrescente";
    ViewDetails => "Details", "Dettagli";
    ViewIcons => "Icons", "Icone";
    ViewColumns => "Columns", "Colonne";
    MenuDetailsPane => "Details pane", "Pannello dei dettagli";
    MenuHiddenFiles => "Hidden files", "File nascosti";
    MenuNavPane => "Navigation pane", "Barra laterale";
    MenuTagColumn => "Tags column", "Colonna dei tag";
    MenuLayoutAuto => "Layout: automatic", "Disposizione: automatica";
    MenuLayoutPanels => "Layout: panels", "Disposizione: pannelli";
    MenuLayoutClassic => "Layout: classic", "Disposizione: classica";
    MenuCheckUpdates => "Check for updates", "Cerca aggiornamenti";
    MenuAbout => "About rada", "Informazioni su rada";
    MenuNew => "New", "Nuovo";
    MenuSortBy => "Sort by", "Ordina per";
    MenuView => "View", "Vista";
    MenuTags => "Tags", "Tag";
    MenuNewTag => "New tag…", "Nuovo tag…";
    MenuRemoveAllTags => "Remove all tags", "Togli tutti i tag";
    WhatChangeTags => "change tags", "cambiare i tag";

    // ---- places and sections of the sidebar
    PlaceTrash => "Trash", "Cestino";
    PlaceRecent => "Recent", "Recenti";
    SecPinned => "PINNED", "PREFERITI";
    SecDevices => "DEVICES", "DISPOSITIVI";
    SecTags => "TAGS", "TAG";
    DiskSystem => "System", "Sistema";

    // ---- the tab bar
    UpdateTo => "Update to {0}", "Aggiorna a {0}";
    HelpMenu => "Help", "Aiuto";

    // ---- the bar at the top
    BarPlaceholder => "Search files or type a command…", "Cerca file o digita un comando…";
    BarCommand => "command", "comando";
    BarGoTo => "go to", "vai a";
    HomeLabel => "Home", "Home";

    // ---- the command bar
    BtnCompress => "Compress", "Comprimi";
    BtnDelete => "Delete", "Elimina";
    BtnSort => "Sort", "Ordina";

    // ---- the list and the grid
    ListLoading => "{0} loading…", "{0} caricamento…";
    ListNoMatch => "Nothing here matches the search", "Nessun elemento corrisponde alla ricerca";
    ListEmpty => "This folder is empty", "Questa cartella è vuota";
    ListOnlyHidden => "Only hidden files here — press . to show them", "Qui ci sono solo file nascosti — premi . per mostrarli";
    GridTooSmall => "too small for icons", "troppo piccolo per le icone";

    // ---- the status bar and the key hints
    HRun => "run", "esegui";
    HCancel => "cancel", "chiudi";
    HOk => "ok", "ok";
    HGo => "go", "vai";
    HClose => "close", "chiudi";
    HChoose => "choose", "scegli";
    HConfirm => "confirm", "conferma";
    HStop => "stop", "interrompi";
    HOpen => "open", "apri";
    HAlways => "always", "sempre";
    HQuit => "quit", "esci";
    HStay => "stay", "resta";
    HComplete => "complete", "completa";
    HSuggestions => "suggestions", "suggerimenti";
    HKeep => "keep", "tieni";
    HClear => "clear", "cancella";
    HClearSearch => "clear search", "cancella la ricerca";
    HList => "list", "elenco";
    HPin => "pin", "fissa";
    HUnpin => "unpin", "stacca";
    HHide => "hide", "nascondi";
    HCopyOut => "copy out", "copia fuori";
    HExtractAll => "extract all", "estrai tutto";
    HSelect => "select", "seleziona";
    HBack => "back", "indietro";
    HUndo => "undo", "annulla";
    HHelp => "help", "aiuto";
    HExtract => "extract", "estrai";
    HExtractTo => "extract to…", "estrai in…";
    HCopy => "copy", "copia";
    HPaste => "paste", "incolla";
    HTrash => "trash", "cestino";
    HRename => "rename", "rinomina";
    HNewTab => "new tab", "nuova scheda";
    HCommands => "commands", "comandi";
    HCut => "cut", "taglia";
    HNewFolder => "new folder", "nuova cartella";
    HAddress => "address", "indirizzo";
    HCompress => "compress", "comprimi";
    HDelete => "delete", "elimina";
    HRenameAll => "rename all", "rinomina tutti";
    HDeselect => "clear", "deseleziona";
    ItemsOfTotal => "{0} of {1}", "{0} di {1}";
    SelectedSizeOne => "1 selected · {0}", "1 selezionato · {0}";
    SelectedSizeMany => "{0} selected · {1}", "{0} selezionati · {1}";
    ReadyPaste => "{0} ready to paste", "{0} da incollare";
    ReadyMove => "{0} ready to move", "{0} da spostare";
    HiddenShown => "hidden files shown", "file nascosti visibili";
    FreeSpace => "{0} free", "{0} liberi";
    SpaceUsedOf => "{0} of {1}", "{0} di {1}";
    SpaceUsedWord => "used", "usato";
    // ---- the boxes of the panels view (the names on their top edge)
    PanelNav => "NAV", "NAV";
    PanelSpace => "SPACE", "SPAZIO";
    PanelPreview => "PREVIEW", "ANTEPRIMA";
    PanelDetails => "DETAILS", "DETTAGLI";
    PanelPath => "PATH", "PERCORSO";
    NavMore => "… {0} more", "… altre {0}";
    NavMoreDisks => "… {0} more", "… altri {0}";
    // ---- git, in the panels view
    GitLabel => "Git", "Git";
    GitModified => "modified", "modificato";
    GitNew => "new", "nuovo";
    GitClean => "no changes", "nessuna modifica";

    // ---- the frame
    TooSmall => "terminal too small", "terminale troppo piccolo";
    EnlargeNotice => "Enlarge the window for the full layout (now {0}×{1})  Esc: hide", "Ingrandisci la finestra per vedere tutto (ora {0}×{1})  Esc: nascondi";
    ProgLeft => "{0} left", "ne restano {0}";
    ProgCancel => "cancel", "interrompi";

    // ---- the preview
    PvEmptyFile => "Empty file", "File vuoto";
    PvEmptyFolder => "Empty folder", "Cartella vuota";
    PvBeginningOnly => "Beginning of the file only", "Solo l'inizio del file";
    PvBackToSummary => "back to the summary", "torna al riepilogo";
    PvExecutable => "Executable", "Eseguibile";
    PvInterpreter => "Interpreter", "Interprete";
    PvSize => "Size", "Dimensione";
    PvSizeBytes => "{0} · {1} bytes", "{0} · {1} byte";
    PvModified => "Modified", "Modificato";
    PvCreated => "Created", "Creato";
    PvAccessed => "Accessed", "Ultimo accesso";
    PvPermissions => "Permissions", "Permessi";
    PvShowHex => "show the hex dump", "mostra il dump esadecimale";
    PvFormat => "Format", "Formato";
    PvAtLeast => "at least ", "almeno ";
    PvContains => "Contains", "Contiene";
    PvPacked => "Packed", "Compresso";
    PvUnpacked => "Unpacked", "Decompresso";
    PvRatio => "Ratio", "Rapporto";
    PvRatioValue => "{0} to 1", "{0} a 1";
    PvOpenLikeFolder => "open like a folder", "apri come una cartella";
    PvExtract => "extract", "estrai";
    PvPointsToFile => "points to a file", "punta a un file";
    PvPointsToFolder => "points to a folder", "punta a una cartella";
    PvBrokenLink => "broken link: the target does not exist", "collegamento interrotto: la destinazione non esiste";
    PvCircularLink => "circular link", "collegamento circolare";
    PvBeginningOnlyDot => " · beginning only", " · solo l'inizio";
    PvLongLinesCut => " · {0} long line(s) cut", " · {0} riga/righe lunghe tagliate";
    PvHexDump => "hex dump · first {0} bytes", "dump esadecimale · primi {0} byte";
    PvMoreNotListed => " · more not listed", " · altri non elencati";
    PvShownOne => "{0} shown{1}", "{0} mostrato{1}";
    PvShownMany => "{0} shown{1}", "{0} mostrati{1}";
    PvSymlink => "Symbolic link", "Collegamento simbolico";
    PvPdfPages => "PDF · {0} · {1}", "PDF · {0} · {1}";
    PvPagesUnknown => "pages unknown", "pagine sconosciute";
    PvSizeUnknown => "size unknown", "dimensione sconosciuta";
    PvTitleLine => "title: {0}", "titolo: {0}";
    PvAuthorLine => "author: {0}", "autore: {0}";
    PvModifiedLine => "modified {0}", "modificato {0}";
    PvDecoding => "{0} decoding…", "{0} decodifica…";
    PvImagesOff => "image rendering is off (--images)", "le immagini sono disattivate (--images)";
    PvPdfTooLarge => "PDF too large to preview", "PDF troppo grande per l'anteprima";
    PvImageTooLarge => "image too large to preview", "immagine troppo grande per l'anteprima";
    PvCannotPdf => "cannot show this PDF", "impossibile mostrare questo PDF";
    PvCannotImage => "cannot show this image", "impossibile mostrare questa immagine";
    PvFirstPageText => "{0} · first page as text", "{0} · prima pagina come testo";

    // ---- the preview (passwords)
    PvPwOne => "1 item is protected by a password", "1 elemento è protetto da una password";
    PvPwMany => "{0} items are protected by a password", "{0} elementi sono protetti da una password";

    // ---- notifications
    NCopyOne => "1 item copied to {0}", "1 elemento copiato in {0}";
    NCopyMany => "{0} items copied to {1}", "{0} elementi copiati in {1}";
    NMoveOne => "1 item moved to {0}", "1 elemento spostato in {0}";
    NMoveMany => "{0} items moved to {1}", "{0} elementi spostati in {1}";
    NTrashOne => "1 item moved to the trash", "1 elemento spostato nel cestino";
    NTrashMany => "{0} items moved to the trash", "{0} elementi spostati nel cestino";
    NRenameOne => "Renamed to {0}", "Rinominato in {0}";
    NRenameMany => "{0} items renamed", "{0} elementi rinominati";
    NCompressOne => "1 item compressed into {0}", "1 elemento compresso in {0}";
    NCompressMany => "{0} items compressed into {1}", "{0} elementi compressi in {1}";
    NExtractOne => "1 item extracted to {0}", "1 elemento estratto in {0}";
    NExtractMany => "{0} items extracted to {1}", "{0} elementi estratti in {1}";
    NTagsOne => "Tags changed on 1 item", "Tag modificati su 1 elemento";
    NTagsMany => "Tags changed on {0} items", "Tag modificati su {0} elementi";
    NFolderCreated => "Folder created: {0}", "Cartella creata: {0}";
    NFileCreated => "File created: {0}", "File creato: {0}";
    NDeleteOne => "1 item deleted permanently", "1 elemento eliminato definitivamente";
    NDeleteMany => "{0} items deleted permanently", "{0} elementi eliminati definitivamente";
    NLeftOutOne => ", 1 skipped", ", 1 saltato";
    NLeftOutMany => ", {0} skipped", ", {0} saltati";
    NUndone => "Undone", "Annullato";
    NDoneDefault => "{0}: done", "{0}: fatto";
    NDoneKept => "{0}: done, some items were kept", "{0}: fatto, alcuni elementi sono rimasti";
    NDoneNotKept => "{0}: done, but some attributes could not be kept", "{0}: fatto, ma alcuni attributi non si sono potuti conservare";
    NProblems => "{0}: finished with {1} problem(s)", "{0}: concluso con {1} problema/i";
    NStopped => "{0}: stopped; press u to undo what was done", "{0}: fermato; premi u per annullare ciò che è stato fatto";
    NCancelled => "{0}: cancelled; press u to undo what was done", "{0}: interrotto; premi u per annullare ciò che è stato fatto";
    NInterrupted => "{0}: interrupted; press u to undo what was done", "{0}: interrotto; premi u per annullare ciò che è stato fatto";
    NSkippedFolderOne => "1 more skipped because its folder failed", "1 saltato in più perché la sua cartella ha dato errore";
    NSkippedFolderMany => "{0} more skipped because their folder failed", "{0} saltati in più perché la loro cartella ha dato errore";
    NNotPreserved => "not preserved: {0} — {1}, for example {2}", "non conservati: {0} — {1}, per esempio {2}";
    NJournalErrorsOne => "1 step could not be written to the journal and may not be undoable", "1 passaggio non si è potuto scrivere nel registro e potrebbe non essere annullabile";
    NJournalErrorsMany => "{0} steps could not be written to the journal and may not be undoable", "{0} passaggi non si sono potuti scrivere nel registro e potrebbero non essere annullabili";
    NoticeShow => "Show", "Mostra";
    NoticeClose => "Close", "Chiudi";

    // ---- notices
    TJournalDown => "journal unavailable: operations cannot be undone this session", "registro non disponibile: in questa sessione le operazioni non si possono annullare";
    TTagViewRefuse => "this list is the items tagged \"{0}\", not a folder — cannot {1} here", "questo elenco contiene gli elementi con il tag \"{0}\", non è una cartella — qui non si può {1}";
    TRecentRefuse => "this list is the files used lately, not a folder — cannot {0} here", "questo elenco contiene i file usati di recente, non è una cartella — qui non si può {0}";
    TShowInFolderHere => "this is already a folder: nothing to show", "questa è già una cartella: non c'è altro da mostrare";
    TAtTop => "already at the top", "sei già in cima";
    TInArchive => "inside an archive: copy ({0}) it to a folder, or extract with {1}", "dentro un archivio: copialo ({0}) in una cartella, oppure estrai con {1}";
    TOpening => "opening {0}", "apro {0}";
    TNotPackage => "this is not a package", "questo non è un pacchetto";
    TQuickLookMac => "Quick Look is a macOS feature", "Anteprima rapida è una funzione di macOS";
    TLiveOff => "live updates off: {0}", "aggiornamenti in tempo reale disattivati: {0}";
    TNothingUndo => "nothing to undo", "niente da annullare";
    TNothingRedo => "nothing to redo (redo only follows an undo, until another operation)", "niente da ripetere (si può ripetere solo dopo un annullamento, finché non c'è un'altra operazione)";
    TBusy => "an operation is running; wait or press Esc to cancel it", "un'operazione è in corso: aspetta o premi Esc per interromperla";
    TArchiveReadOnly => "an archive is read-only — cannot {0}; copy ({1}) items out to a folder instead", "un archivio è in sola lettura — non si può {0}; copia ({1}) gli elementi in una cartella";
    TNothingCopied => "nothing copied yet (y = copy, x = cut)", "non hai ancora copiato nulla (y = copia, x = taglia)";
    TYankCopiedOne => "{0} copied — go to the destination and press {1}", "{0} copiato — vai alla destinazione e premi {1}";
    TYankCopiedMany => "{0} copied — go to the destination and press {1}", "{0} copiati — vai alla destinazione e premi {1}";
    TYankCutOne => "{0} cut — go to the destination and press {1}", "{0} tagliato — vai alla destinazione e premi {1}";
    TYankCutMany => "{0} cut — go to the destination and press {1}", "{0} tagliati — vai alla destinazione e premi {1}";
    TSortedBy => "sorted by {0} {1}", "ordinati per {0} {1}";
    TBookmarked => "folder bookmarked — it is in the sidebar and in Ctrl+P", "cartella tra i preferiti — è nella barra laterale e in Ctrl+P";
    TBookmarkRemoved => "bookmark removed", "preferito rimosso";
    TCannotUndoThis => "this operation cannot be undone (permanent, already undone, or nothing to undo)", "questa operazione non si può annullare (definitiva, già annullata, o niente da annullare)";
    WhatCreateFolder => "create a folder", "creare una cartella";
    WhatCreateFile => "create a file", "creare un file";
    WhatOpenWithProgram => "open with a program", "aprire con un programma";
    WhatPasteHere => "paste here", "incollare qui";
    WhatPaste => "paste", "incollare";
    WhatCut => "cut", "tagliare";
    WhatDelete => "delete", "eliminare";
    WhatRename => "rename", "rinominare";
    WhatBookmark => "bookmark it", "aggiungerla ai preferiti";
    WhatCompressHere => "compress from here", "comprimere da qui";
    WhatQuickLook => "preview with Quick Look", "vedere l'anteprima rapida";
    SortKeyName => "name", "nome";
    SortKeySize => "size", "dimensione";
    SortKeyDate => "date", "data";
    SortKeyType => "type", "tipo";
    KeyCopyWord => "copy", "copia";
    KeyExtractWord => "extract", "estrai";
    KeyPasteWord => "paste", "incolla";
    TCancelling => "cancelling…", "interruzione in corso…";
    THexOnlyBinary => "the hex dump is available for binary files", "il dump esadecimale è disponibile per i file binari";
    TDetailsOnNarrow => "details pane on — this terminal is too narrow to show it", "pannello dei dettagli attivo — questo terminale è troppo stretto per mostrarlo";
    TOpenWithFiles => "Open with… is for files", "Apri con… vale per i file";
    TPathCopied => "path copied", "percorso copiato";
    TPathsCopied => "paths copied", "percorsi copiati";
    TOpeningWith => "opening {0} with {1}{2}", "apro {0} con {1}{2}";
    TSettingsOpened => "System Settings opened — allow it there, then try the folder again", "Impostazioni di Sistema aperte — consenti lì l'accesso, poi riprova con la cartella";
    TSidebarOnNarrow => "sidebar on — this terminal is too narrow to show it", "barra laterale attiva — questo terminale è troppo stretto per mostrarla";
    TSidebarNarrow => "this terminal is too narrow for the sidebar", "questo terminale è troppo stretto per la barra laterale";
    TOnlyBookmarksRemovable => "only bookmarks can be removed from the sidebar (a disk is ejected with this key)", "dalla barra laterale si possono togliere solo i preferiti (un disco si espelle con questo tasto)";
    TSidebarFocus => "the sidebar has the focus — {0} goes back to the list", "il focus è sulla barra laterale — {0} torna all'elenco";
    TTabLimit => "{0} tabs is the limit", "{0} schede è il limite";
    TCannotOpenSettings => "cannot open the system settings: {0}", "impossibile aprire le impostazioni di sistema: {0}";
    TCannotEject => "this one cannot be ejected", "questo non si può espellere";
    TEjected => "{0} was ejected: you can unplug it", "{0} è stato espulso: puoi scollegarlo";
    TExtractPartHint => "to extract part of an archive somewhere else: copy it ({0}), go there and paste ({1})", "per estrarre una parte di un archivio altrove: copiala ({0}), vai lì e incolla ({1})";
    TSelectArchive => "select an archive to extract (zip, tar, 7z, rar, .gz, .xz, .zst…)", "seleziona un archivio da estrarre (zip, tar, 7z, rar, .gz, .xz, .zst…)";
    TOneArchive => "extract one archive at a time", "estrai un archivio alla volta";
    TUpToDate => "rada is up to date", "rada è aggiornato";
    TCannotLookUpdates => "cannot look for updates: {0}", "impossibile cercare aggiornamenti: {0}";
    TUpdateFailed => "the update did not work", "l'aggiornamento non è riuscito";
    TUpdateRunning => "an update is already running", "un aggiornamento è già in corso";
    TLookingNewer => "looking for a newer version…", "cerco una versione più recente…";
    TUpdatesNA => "updates are not available here", "qui gli aggiornamenti non sono disponibili";
    TUpdatingBackground => "updating in the background", "aggiornamento in corso in background";

    // ---- messages of the app
    PlanningGeneric => "Planning", "Preparo il piano";
    PlanningRedo => "Planning redo", "Preparo il piano per ripetere";
    PlanningUndo => "Planning undo", "Preparo il piano per annullare";
    PlanningRename => "Planning rename", "Preparo il piano per rinominare";
    PlanningCompress => "Planning compression", "Preparo il piano per comprimere";
    PlanningCopy => "Planning copy", "Preparo il piano per copiare";
    ReadingArchive => "Reading the archive", "Leggo l'archivio";
    TFontNotInstalled => "the font was not installed: {0}", "il font non è stato installato: {0}";
    TOpeningWithNote1 => " — rada will use it for this kind of file", " — rada lo userà per questo tipo di file";
    TOpeningWithNote2 => " — rada goes back to the system's choice for this kind of file", " — rada torna alla scelta del sistema per questo tipo di file";
    SideFree => "{0} · {1} free", "{0} · {1} liberi";
    SideMenuOpen => "Open", "Apri";
    PreviewOnDevice => "on a phone or camera: not read, so that the device does not have to send the whole file", "su un telefono o una fotocamera: non letto, perché il dispositivo non debba inviare tutto il file";
    DevMissingGio => "gio was not found: install gvfs (and gvfs-mtp for phones) to open devices from rada", "gio non è stato trovato: installa gvfs (e gvfs-mtp per i telefoni) per aprire i dispositivi da rada";
    DevMissingUdisks => "udisksctl was not found: install udisks2 to mount disks from rada", "udisksctl non è stato trovato: installa udisks2 per montare i dischi da rada";
    DevNoMtp => "this system cannot open phones: install gvfs-mtp (on Arch: pacman -S gvfs-mtp)", "questo sistema non sa aprire i telefoni: installa gvfs-mtp (su Arch: pacman -S gvfs-mtp)";
    DevBusy => "another program is holding the device (a photo or sync app, or another file manager): close it and try again", "un altro programma sta usando il dispositivo (un'app di foto o di sincronizzazione, o un altro gestore di file): chiudilo e riprova";
    DevLocked => "the phone did not answer: unlock it, choose \"File transfer\" in its USB notification and try again", "il telefono non ha risposto: sbloccalo, scegli \"Trasferimento file\" nella notifica USB e riprova";
    DevNotAuthorized => "the system asks for a password to do this: use your desktop's disk tool", "il sistema chiede una password per farlo: usa lo strumento dei dischi del tuo desktop";
    DevUnmountBusy => "the device is busy: a program still has something open on it. Close it and try again.", "il dispositivo è occupato: un programma ha ancora qualcosa di aperto. Chiudilo e riprova.";
    DevMountFailed => "it could not be mounted: {0}", "non si è potuto montare: {0}";
    DevUnmountFailed => "the system could not unmount it: {0}", "il sistema non è riuscito a smontarlo: {0}";
    DevTimedOut => "the device did not answer in time", "il dispositivo non ha risposto in tempo";
    DevNotMounted => "not mounted", "non montato";
    BtnMount => "Mount", "Monta";
    SideMenuMount => "Mount", "Monta";
    TMounting => "mounting {0}…", "montaggio di {0}…";
    TMounted => "{0} is mounted", "{0} è montato";
    SideMenuEject => "Eject", "Espelli";
    SideMenuPin => "Pin to navigation", "Fissa nella barra laterale";
    SideMenuUnpin => "Unpin from navigation", "Stacca dalla barra laterale";
    AboutTitle => "About rada", "Informazioni su rada";
    AboutVersion => "Version", "Versione";
    AboutChannel => "Channel", "Canale";
    AboutMode => "Mode", "Modo";
    AboutChecks => "Checks", "Controlli";
    AboutProgram => "Program", "Programma";
    AboutUpdates => "Updates", "Aggiornamenti";
    AboutSource => "Source", "Sorgente";
    AboutPre => "pre-releases", "versioni di prova";
    AboutStable => "stable releases", "versioni stabili";
    AboutChosenVersion => "chosen from the version", "scelto in base alla versione";
    AboutSetInConfig => "set in the configuration", "impostato nella configurazione";
    AboutChannelValue => "{0} ({1})", "{0} ({1})";
    AboutBinary => "a program downloaded from the releases — it replaces itself", "un programma scaricato dalle versioni — si sostituisce da solo";
    AboutSourceMode => "built from the git checkout in {0} — it pulls and builds", "compilato dalla copia git in {0} — si aggiorna e si ricompila";
    AboutDaily => "once a day in the background", "una volta al giorno in background";
    AboutOnlyAsked => "only when asked (check = false)", "solo su richiesta (check = false)";
    AboutNA => "not available here", "non disponibili qui";
    UpDoneRestart => "rada {0} is installed: start rada again to use it", "rada {0} è installato: riavvia rada per usarlo";
    UpStarting => "starting the update to {0}", "avvio l'aggiornamento a {0}";
    UpPress => " — press {0} to {1}", " — premi {0} per {1}";
    UpWordUpdate => "update", "aggiornare";
    UpWordSeeWhy => "see why", "vedere perché";
    UpNewVersion => "New version {0} available{1}", "Nuova versione {0} disponibile{1}";
    UpUpdatingStep => "Updating: {0} ({1})", "Aggiornamento: {0} ({1})";
    UpUpdating => "Updating… ({0})", "Aggiornamento… ({0})";
    UpInstalledLong => "rada {0} installed — start rada again to use it", "rada {0} installato — riavvia rada per usarlo";
    UpRestart => "restart rada", "riavvia rada";
    UpFailedLong => "The update did not work{0}", "L'aggiornamento non è riuscito{0}";
    UpFailedShort => "Update failed{0}", "Aggiornamento fallito{0}";
    TCannotOpenPath => "cannot open {0}: {1}", "impossibile aprire {0}: {1}";

    // ---- planning
    PlanningMove => "Planning move", "Preparo il piano per spostare";

    // ---- more of the app
    TCannotOpen => "cannot open {0}: {1}", "impossibile aprire {0}: {1}";
    TQuickLookErr => "Quick Look: {0}", "Anteprima rapida: {0}";
    ResultCouldNot => "Could not go on", "Non si è potuto continuare";
    ResultRecovered => "\"{0}\" was interrupted last time: {1} leftover temporary file(s) removed, {2} finished step(s) added to the history", "\"{0}\" era stata interrotta l'ultima volta: {1} file temporanei rimasti eliminati, {2} passaggi conclusi aggiunti alla cronologia";
    JournalDamaged => "Line {0} of the history file {1} cannot be read. Operations from that point on are neither settled nor undone by rada; the ones before it are fine. Repair or remove that line, or restore the file from a backup.", "La riga {0} del file della cronologia {1} non è leggibile. Le operazioni da quel punto in poi non vengono né sistemate né annullate da rada; quelle prima sono a posto. Correggi o togli quella riga, oppure ripristina il file da una copia di riserva.";
    ResultOpenHistory => "Open the history to undo what was done before the interruption.", "Apri la cronologia per annullare ciò che era stato fatto prima dell'interruzione.";
    ResultInterruptedTitle => "An earlier operation was interrupted", "Un'operazione precedente è stata interrotta";
    PalNotResponding => " · not responding", " · non risponde";
    PalFreeDetail => " · {0} free", " · {0} liberi";

    // ---- new actions
    LViewColumns => "View as columns", "Vista a colonne";
    LToggleTags => "Show / hide the Tag column", "Mostra / nascondi la colonna dei tag";

    // ---- demo disks
    DiskData => "Data", "Dati";
    DiskUsb => "USB drive", "Chiavetta";

    // ---- the command line: help
    CliAbout => "A safe harbor for your files: a terminal file manager where every operation shows a plan first, can be undone, and keeps going when one file fails.", "Un porto sicuro per i tuoi file: un file manager da terminale in cui ogni operazione mostra prima un piano, si può annullare e va avanti se un file dà errore.";
    CliUsage => "Usage:", "Uso:";
    CliHeadingCommands => "Commands", "Comandi";
    CliHeadingArguments => "Arguments", "Argomenti";
    CliHeadingOptions => "Options", "Opzioni";
    CliPlaceholderOptions => "OPTIONS", "OPZIONI";
    CliPlaceholderCommand => "COMMAND", "COMANDO";
    CliPlaceholderPath => "PATH", "PERCORSO";
    CliHelpFlag => "Print help", "Mostra l'aiuto";
    CliVersionFlag => "Print version", "Mostra la versione";
    CliHelpSubcommand => "Print this message or the help of the given subcommand(s)", "Mostra questo messaggio o l'aiuto dei sottocomandi indicati";
    CliCmdSetup => "Install or remove things rada needs outside its own folders.", "Installa o toglie ciò che serve a rada fuori dalle sue cartelle.";
    CliCmdUpdate => "Look for a newer rada and install it. Reads GitHub's public list of releases and sends nothing but `User-Agent: rada/<version>`.", "Cerca una versione più nuova di rada e la installa. Legge l'elenco pubblico delle versioni di GitHub e non manda altro che `User-Agent: rada/<versione>`.";
    CliCmdDoctor => "Say what rada found out about this terminal, and which profile it will use.", "Dice che cosa rada ha scoperto di questo terminale e quale profilo userà.";
    CliSetupDesktop => "Add rada to the application menu (Linux), make Rada.app in ~/Applications (macOS) or a Start menu shortcut (Windows), for this user only.", "Aggiunge rada al menu delle applicazioni (Linux), crea Rada.app in ~/Applications (macOS) o un collegamento nel menu Start (Windows), solo per questo utente.";
    CliSetupFont => "Download and install JetBrainsMono Nerd Font Mono for this user (a fixed release, checked against its checksums; nothing needs administrator rights), so that the icons draw.", "Scarica e installa JetBrainsMono Nerd Font Mono per questo utente (una versione precisa, controllata con le sue somme di controllo; non servono diritti di amministratore), così le icone si disegnano.";
    CliArgRemoveDesktop => "Take away exactly what a previous setup installed.", "Toglie esattamente ciò che un'installazione precedente ha messo.";
    CliArgRemoveFont => "Take away what a previous setup installed.", "Toglie ciò che un'installazione precedente ha messo.";
    CliArgDefaultFm => "Also make rada the default program for folders (Linux: the previous one is remembered and `--remove` restores it; Windows: for your account only, shows what it writes in the registry and asks first, and `--remove` puts it back).", "Rende rada anche il programma predefinito per le cartelle (Linux: quello di prima viene ricordato e `--remove` lo rimette; Windows: solo per il tuo account, mostra che cosa scrive nel registro e chiede conferma, e `--remove` rimette tutto).";
    CliArgDesktopYes => "With --default-file-manager on Windows: do not ask before writing the registry.", "Con --default-file-manager su Windows: non chiede conferma prima di scrivere nel registro.";
    CliArgDesktopLauncher => "Also put a launcher on the Desktop (macOS and Windows).", "Mette anche un collegamento sulla Scrivania (macOS e Windows).";
    CliArgCheck => "Only say whether a newer version exists, and what is in it.", "Dice solo se esiste una versione più nuova e che cosa contiene.";
    CliArgRollback => "Go back to the version that was running before the last update.", "Torna alla versione che girava prima dell'ultimo aggiornamento.";
    CliArgYes => "Do not ask before installing.", "Non chiede conferma prima di installare.";
    CliArgPath => "Folder to open (default: the current folder). A folder named like a command (`setup`) needs a leading `./`.", "Cartella da aprire (predefinita: quella corrente). Una cartella che si chiama come un comando (`setup`) vuole `./` davanti.";
    CliArgSpawn => "Open rada in a new terminal window (used by the desktop launcher entry).", "Apre rada in una nuova finestra di terminale (lo usa la voce del menu delle applicazioni).";
    CliArgIcons => "Icons: `auto` (default: the best set the terminal's font can draw; Nerd Font symbols if the terminal uses such a font, or is Ghostty), `nerd`, `unicode-icons`, `unicode-std` or `none`.", "Icone: `auto` (predefinito: il meglio che il carattere del terminale sa disegnare; simboli Nerd Font se il terminale ne usa uno, o è Ghostty), `nerd`, `unicode-icons`, `unicode-std` oppure `none`.";
    CliArgProfile => "How much is asked of the terminal: `auto` (default), `modern` (UTF-8, colours, mouse, pictures) or `basic` (ASCII, 16 colours, keyboard only).", "Quanto si chiede al terminale: `auto` (predefinito), `modern` (UTF-8, colori, mouse, immagini) oppure `basic` (ASCII, 16 colori, solo tastiera).";
    CliArgImages => "Image previews: `auto` (default), `halfblocks`, `kitty`, `sixel`, `iterm2` or `off`.", "Anteprime delle immagini: `auto` (predefinito), `halfblocks`, `kitty`, `sixel`, `iterm2` oppure `off`.";
    CliArgTheme => "Colour theme: `auto` (default), rada, catppuccin or tokyo-night (these follow the terminal's background), or one fixed variant: rada-dark, rada-light, catppuccin-mocha, catppuccin-latte, tokyo-night-dark, tokyo-night-day, nebbia-light, carta.", "Tema di colori: `auto` (predefinito), rada, catppuccin o tokyo-night (seguono lo sfondo del terminale), oppure una variante precisa: rada-dark, rada-light, catppuccin-mocha, catppuccin-latte, tokyo-night-dark, tokyo-night-day, nebbia-light, carta.";
    CliArgAppearance => "Which variant a theme name takes: `auto` (ask the terminal), `light` or `dark`.", "Quale variante prende il nome di un tema: `auto` (lo chiede al terminale), `light` oppure `dark`.";
    CliArgNoMouse => "Do not capture the mouse (the terminal keeps selecting text).", "Non cattura il mouse (il terminale continua a selezionare il testo).";
    CliArgKeymap => "Key bindings: `vim+classic` (default), `vim` or `classic`.", "Scorciatoie da tastiera: `vim+classic` (predefinito), `vim` oppure `classic`.";
    CliArgRequest => "Plan an operation given as JSON (a file, or `-` for stdin) and show it in the usual confirmation window. See `rada --schema request`.", "Pianifica un'operazione data come JSON (un file, o `-` per lo standard input) e la mostra nella solita finestra di conferma. Vedi `rada --schema request`.";
    CliArgSchema => "Print the JSON Schema of `request` (what `--request` accepts) or `plan`, then quit.", "Stampa lo schema JSON di `request` (ciò che `--request` accetta) o di `plan`, poi esce.";
    CliArgHidden => "Show hidden files at startup.", "Mostra i file nascosti all'avvio.";
    CliArgCwdFile => "Write the last folder to FILE on exit (for the shell `cd` helper).", "Scrive l'ultima cartella in FILE all'uscita (per l'aiuto `cd` della shell).";
    CliArgInit => "Print the shell helper that changes directory on exit, then quit.", "Stampa l'aiuto per la shell che cambia cartella all'uscita, poi esce.";

    // ---- the command line: errors and what the commands say
    MNoDirs => "cannot locate your home and data folders", "impossibile trovare la tua cartella personale e quelle dei dati";
    MNoExe => "cannot find the rada executable", "impossibile trovare l'eseguibile di rada";
    MNoCwd => "cannot read the current folder", "impossibile leggere la cartella corrente";
    MCannotCreate => "cannot create {0}", "impossibile creare {0}";
    MCannotWrite => "cannot write {0}", "impossibile scrivere {0}";
    MCannotRead => "cannot read {0}", "impossibile leggere {0}";
    MCannotOpen => "cannot open {0}", "impossibile aprire {0}";
    MCannotRemove => "cannot remove {0}", "impossibile togliere {0}";
    MCannotReplace => "cannot replace {0}", "impossibile sostituire {0}";
    MCannotUpdate => "cannot update {0}", "impossibile aggiornare {0}";
    MCannotStart => "cannot start {0}", "impossibile avviare {0}";
    MCannotRunnable => "cannot make {0} runnable", "impossibile rendere eseguibile {0}";
    MCannotRegisterFont => "cannot register {0} for your account", "impossibile registrare {0} per il tuo account";
    MNotAvailable => "{0} is not available", "{0} non è disponibile";
    MProgramFailed => "{0} failed: {1}", "{0} non è riuscito: {1}";
    MNoTerminal => "no terminal emulator found: install one, or set $TERMINAL (for example TERMINAL=kitty)", "nessun emulatore di terminale trovato: installane uno o imposta $TERMINAL (per esempio TERMINAL=kitty)";
    MBadProfile => "unknown profile \"{0}\" (use auto, modern or basic)", "profilo sconosciuto \"{0}\" (usa auto, modern o basic)";
    MBadIcons => "unknown icon set \"{0}\" (use auto, {1})", "set di icone sconosciuto \"{0}\" (usa auto, {1})";
    MBadTheme => "unknown theme \"{0}\" (use auto, rada, catppuccin, tokyo-night, or one of {1})", "tema sconosciuto \"{0}\" (usa auto, rada, catppuccin, tokyo-night, oppure uno tra {1})";
    MBadAppearance => "unknown appearance \"{0}\" (use auto, light or dark)", "aspetto sconosciuto \"{0}\" (usa auto, light o dark)";
    MBadImages => "unknown image mode \"{0}\" (use auto, halfblocks, kitty, sixel, iterm2 or off)", "modo immagini sconosciuto \"{0}\" (usa auto, halfblocks, kitty, sixel, iterm2 oppure off)";
    MBadKeymap => "unknown keymap \"{0}\" (use {1})", "scorciatoie sconosciute \"{0}\" (usa {1})";
    MBadLayout => "unknown layout \"{0}\" (use auto, panels, classic or compact)", "disposizione sconosciuta \"{0}\" (usa auto, pannelli, classico o compact)";
    MBadView => "unknown view \"{0}\" (use details, icons or columns)", "vista sconosciuta \"{0}\" (usa details, icons o columns)";
    MBadNav => "unknown nav \"{0}\" (use path or tree)", "nav sconosciuto \"{0}\" (usa percorso o albero)";
    MBadDensity => "unknown sidebar_density \"{0}\" (use {1})", "sidebar_density sconosciuta \"{0}\" (usa {1})";
    MBadDates => "unknown dates \"{0}\" (use relative or absolute)", "formato delle date sconosciuto \"{0}\" (usa relative o absolute)";
    MBadFolderSizes => "unknown folder_sizes \"{0}\" (use auto, items or off)", "folder_sizes sconosciuto \"{0}\" (usa auto, items oppure off)";
    MRequestStdin => "cannot read the request from stdin", "impossibile leggere la richiesta dallo standard input";
    MRequestBad => "the request is not valid (see `rada --schema request`)", "la richiesta non è valida (vedi `rada --schema request`)";
    MTerminalError => "terminal error", "errore del terminale";
    MPressAnyKey => "Press any key to close this window.", "Premi un tasto per chiudere questa finestra.";
    MFontInstalled => "{0} is installed: choose it as the font of your terminal, then start rada again", "{0} è installato: sceglilo come carattere del terminale, poi riavvia rada";
    SayInstalled => "installed {0}", "installato {0}";
    SayRemoved => "removed {0}", "tolto {0}";
    SayCouldNotRemove => "could not remove {0}: {1}", "impossibile togliere {0}: {1}";
    SayMade => "made {0}", "creato {0}";
    SayNothingToRemove => "nothing to remove", "non c'è nulla da togliere";
    SayDbUpdated => "updated the desktop database", "aggiornato l'archivio delle voci del desktop";
    SayDbNote => "note: could not update the desktop database ({0}); the entry appears after the next login", "nota: impossibile aggiornare l'archivio delle voci del desktop ({0}); la voce compare dopo il prossimo accesso";
    SayAlreadyDefault => "rada is already the default file manager", "rada è già il gestore di file predefinito";
    SayNowDefaultNone => "rada is now the default file manager (there was none before)", "ora rada è il gestore di file predefinito (prima non ce n'era uno)";
    SayNowDefaultWas => "rada is now the default file manager (it was {0})", "ora rada è il gestore di file predefinito (prima era {0})";
    MXdgSet => "xdg-mime could not set the default file manager", "xdg-mime non è riuscito a impostare il gestore di file predefinito";
    MXdgRestore => "xdg-mime could not restore the previous file manager", "xdg-mime non è riuscito a rimettere il gestore di file di prima";
    SayRestored => "restored {0} as the default file manager", "rimesso {0} come gestore di file predefinito";
    SayDefaultChanged => "the default file manager was changed since; left as it is", "il gestore di file predefinito è stato cambiato nel frattempo; lasciato com'è";
    SayForgotDefault => "removed rada as the default file manager from {0}", "tolto rada come gestore di file predefinito da {0}";
    SayDesktopFlag => "--desktop is for macOS and Windows: on Linux the entry is in the application menu", "--desktop è per macOS e Windows: su Linux la voce è nel menu delle applicazioni";
    MRemoveAndDefault => "--remove and --default-file-manager cannot be used together", "--remove e --default-file-manager non si possono usare insieme";
    MDefaultFmMac => "--default-file-manager is for Linux and Windows: macOS has no setting for it that a program may change", "--default-file-manager è per Linux e Windows: macOS non ha un'impostazione che un programma possa cambiare";
    SayMacLink => "made a link to it on the Desktop: {0}", "creato un collegamento sulla Scrivania: {0}";
    SayMacGhostty => "Rada.app opens rada in Ghostty (in iTerm2 or Terminal if that is not installed). While it runs the Dock shows the terminal's icon: a window belongs to the program that draws it.", "Rada.app apre rada in Ghostty (in iTerm2 o Terminale se non è installato). Mentre gira il Dock mostra l'icona del terminale: una finestra appartiene al programma che la disegna.";
    SayMacWindow => "In Ghostty the window is {0}×{1} and Cmd+C, Cmd+V, Cmd+Backspace and the other Mac shortcuts go to rada.", "In Ghostty la finestra è {0}×{1} e Cmd+C, Cmd+V, Cmd+Backspace e le altre scorciatoie del Mac vanno a rada.";
    MMacNotLink => "{0} exists and is not a link made by rada: left alone", "{0} esiste e non è un collegamento fatto da rada: lasciato com'è";
    MMacLinkOnly => "a link on the Desktop is made on macOS only", "il collegamento sulla Scrivania si fa solo su macOS";
    SayWinProfile => "added the Windows Terminal profile \"rada\": {0}", "aggiunto il profilo \"rada\" a Windows Terminal: {0}";
    SayWinNoTerminal => "Windows Terminal is not installed: the shortcut starts rada.exe directly", "Windows Terminal non è installato: il collegamento avvia direttamente rada.exe";
    SayWinShortcut => "made the shortcut {0}", "creato il collegamento {0}";
    SayWinFont => "for the icons, run `rada setup font` and then `rada setup desktop` again: the profile then asks for {0}", "per le icone, esegui `rada setup font` e poi di nuovo `rada setup desktop`: il profilo allora chiede {0}";
    MWinOnly => "shortcuts are made on Windows only ({0})", "i collegamenti si fanno solo su Windows ({0})";
    MFontChecksum => "{0} does not match its checksum (expected {1}, got {2}); nothing was installed", "{0} non corrisponde alla sua somma di controllo (attesa {1}, trovata {2}); non è stato installato nulla";
    MFontNotAFont => "{0} is not a font file; nothing was installed", "{0} non è un file di caratteri; non è stato installato nulla";
    SayFontAlready => "{0} is already installed in {1}", "{0} è già installato in {1}";
    SayFontDownloading => "downloading {0} {1} ({2} MB) from the Nerd Fonts repository…", "scarico {0} {1} ({2} MB) dal deposito di Nerd Fonts…";
    SayFontCache => "refreshed the font cache", "aggiornata la cache dei caratteri";
    SayFontCacheNote => "note: could not refresh the font cache ({0}); the font appears after the next login", "nota: impossibile aggiornare la cache dei caratteri ({0}); il carattere compare dopo il prossimo accesso";
    SayFontInstalled => "installed {0} in {1}", "installato {0} in {1}";
    SayFontLicence => "the licence (SIL Open Font License 1.1) is beside it: {0}", "la licenza (SIL Open Font License 1.1) è accanto: {0}";
    SayFontChoose => "now choose “{0}” as the font of your terminal, and start rada again: the icons will draw", "ora scegli “{0}” come carattere del terminale e riavvia rada: le icone si disegneranno";
    SayFontUnregister => "could not unregister {0}: {1}", "impossibile togliere {0} dal registro: {1}";

    // ---- the command line: rada doctor
    DocProfile => "profile: {0} ({1})", "profilo: {0} ({1})";
    DocTerminal => "terminal: {0}{1}", "terminale: {0}{1}";
    DocInsideMux => ", inside tmux or screen", ", dentro tmux o screen";
    DocColours => "colours: {0}", "colori: {0}";
    DocColoursNone => "none (NO_COLOR)", "nessuno (NO_COLOR)";
    DocColours24 => "24-bit", "24 bit";
    DocGlyphs => "glyphs: {0}", "simboli: {0}";
    DocPictures => "pictures: {0}", "immagini: {0}";
    DocPicturesOff => "off", "spente";
    DocPicturesBlocks => "half blocks", "mezzi blocchi";
    DocPicturesKitty => "kitty graphics", "grafica kitty";
    DocPicturesIterm => "iTerm2 images", "immagini iTerm2";
    DocPicturesAsk => "the terminal is asked", "si chiede al terminale";
    DocMouse => "mouse: {0}", "mouse: {0}";
    DocClipboard => "clipboard: {0}", "appunti: {0}";
    DocClipboardOsc => "through the terminal (OSC 52)", "tramite il terminale (OSC 52)";
    DocClipboardSystem => "the system's own", "quelli del sistema";
    DocQueries => "terminal queries: {0}", "domande al terminale: {0}";
    DocYes => "yes", "sì";
    DocNo => "no", "no";
    DocIconsChosen => "icons: {0} (set by you: icons / RADA_ICONS)", "icone: {0} (scelte da te: icons / RADA_ICONS)";
    DocIconsNerd => "icons: {0} (the terminal's font is a Nerd Font: {1})", "icone: {0} (il carattere del terminale è un Nerd Font: {1})";
    DocIconsNone => "icons: {0} (no Nerd Font found in the terminal's configuration)", "icone: {0} (nessun Nerd Font nella configurazione del terminale)";
    WhyForced => "set by you (profile / RADA_PROFILE)", "scelto da te (profile / RADA_PROFILE)";
    WhyNoTerm => "TERM is not set", "TERM non è impostato";
    WhyPlain => "TERM={0} is a plain terminal", "TERM={0} è un terminale semplice";
    WhyLinuxConsole => "the Linux console has a small font and 16 colours", "la console Linux ha un carattere piccolo e 16 colori";
    WhyLocale => "the locale {0} is not UTF-8", "la lingua di sistema {0} non è UTF-8";
    WhyModern => "{0} is a modern terminal", "{0} è un terminale moderno";
    WhyAssumed => "assumed", "presunto";
    TermWindowsConsole => "the Windows console", "la console di Windows";
    TermLinuxConsole => "the Linux console", "la console Linux";

    // ---- the command line: updating
    UNoNotes => "(no release notes)", "(nessuna nota di rilascio)";
    UBinary => "It will download the program for this system, check the signature of the release's SHA256SUMS and the program against it, and replace this one (the old one is kept for `rada update --rollback`).", "Scaricherà il programma per questo sistema, controllerà la firma delle SHA256SUMS della versione e il programma con esse, e sostituirà questo (il vecchio resta per `rada update --rollback`).";
    USource => "It will update the git checkout in {0} (`git pull --ff-only`, on branch main with no uncommitted changes), build it (`cargo build --release --locked`), put the new program in place of this one and run `cargo clean`.", "Aggiornerà la copia git in {0} (`git pull --ff-only`, sul ramo main e senza modifiche non salvate), la compilerà (`cargo build --release --locked`), metterà il programma nuovo al posto di questo ed eseguirà `cargo clean`.";
    UNotTerminal => "not a terminal: add --yes to update without being asked", "non è un terminale: aggiungi --yes per aggiornare senza che venga chiesto";
    UPrompt => "{0} [y/N] ", "{0} [s/N] ";
    UGoingBack => "Going back from rada {0} to rada {1}…", "Torno da rada {0} a rada {1}…";
    UDoneBack => "Done: this is rada {0} now. Start rada again to use it.", "Fatto: ora è rada {0}. Riavvia rada per usarla.";
    ULooking => "rada {0} — looking for a newer version…", "rada {0} — cerco una versione più nuova…";
    UNewest => "rada {0} is the newest version for this channel.", "rada {0} è la versione più recente di questo canale.";
    UAvailable => "\nrada {0} is available{1} (this is {2}).\n\nWhat is new:", "\nè disponibile rada {0}{1} (questa è la {2}).\n\nNovità:";
    UPreRelease => " (a pre-release)", " (una versione di prova)";
    UReleaseGone => "the release could not be read again; try once more", "non è stato possibile rileggere la versione; riprova";
    UUpdateNow => "\nUpdate now?", "\nAggiorno ora?";
    ULeft => "Left as it is.", "Lasciato com'è.";
    URestart => "Start rada again to use rada {0}.", "Riavvia rada per usare rada {0}.";
    UFailed => "\nThe update did not work: {0}", "\nL'aggiornamento non è riuscito: {0}";
    UUntouched => "The program that was running is untouched. Log: {0}", "Il programma che girava non è stato toccato. Registro: {0}";
    UpNotVersion => "\"{0}\" is not a version number", "\"{0}\" non è un numero di versione";
    UpNoProgram => "cannot find the program: {0}", "impossibile trovare il programma: {0}";
    UpCannotRead => "cannot read {0}: {1}", "impossibile leggere {0}: {1}";
    UpCannotCreate => "cannot create {0}: {1}", "impossibile creare {0}: {1}";
    UpCannotOpen => "cannot open {0}: {1}", "impossibile aprire {0}: {1}";
    UpCannotWrite => "cannot write {0}: {1}", "impossibile scrivere {0}: {1}";
    UpReadingSums => "reading the checksums", "leggo le somme di controllo";
    UpSumsMissing => "SHA256SUMS does not list {0}", "SHA256SUMS non elenca {0}";
    UpSigMissing => "release {0} has no SHA256SUMS.minisig: it is not signed, so nothing was installed", "la release {0} non ha SHA256SUMS.minisig: non è firmata, quindi non è stato installato nulla";
    UpSigChecking => "checking the signature", "controllo la firma";
    UpSigNoKey => "this copy of rada has no public key to check a signature with (it is not an official build), so nothing was installed", "questa copia di rada non ha una chiave pubblica con cui controllare una firma (non è una build ufficiale), quindi non è stato installato nulla";
    UpSigBad => "the signature of SHA256SUMS is not valid ({0}): the release is not signed by the author, so nothing was installed", "la firma di SHA256SUMS non è valida ({0}): la release non è firmata dall'autore, quindi non è stato installato nulla";
    UpSigTag => "the signature is for \"{0}\", not for \"{1}\" (an old release shown again?), so nothing was installed", "la firma è per \"{0}\", non per \"{1}\" (una vecchia release riproposta?), quindi non è stato installato nulla";
    UpDownloading => "downloading {0}: {1}", "scarico {0}: {1}";
    UpChecking => "checking the download", "controllo il file scaricato";
    UpChecksumBad => "the downloaded file does not match its checksum (expected {0}, got {1}); nothing was installed", "il file scaricato non corrisponde alla sua somma di controllo (attesa {0}, trovata {1}); non è stato installato nulla";
    UpUnpacking => "unpacking", "estraggo";
    UpCannotWriteDir => "rada cannot write in {0}: {1}", "rada non può scrivere in {0}: {1}";
    UpCheckingNew => "checking the new program", "controllo il programma nuovo";
    UpNewSays => "the new program says \"{0}\", not rada {1}; nothing was installed", "il programma nuovo dice \"{0}\", non rada {1}; non è stato installato nulla";
    UpInstalling => "installing", "installo";
    UpInstalledAt => "rada {0} is installed at {1}.", "rada {0} è installata in {1}.";
    UpWentBack => "Went back to rada {0}.", "Tornato a rada {0}.";
    UpArchiveDamaged => "the archive is damaged: {0}", "l'archivio è danneggiato: {0}";
    UpArchiveHuge => "the program in the archive is implausibly large", "il programma nell'archivio è assurdamente grande";
    UpArchiveEmpty => "the program in the archive is empty", "il programma nell'archivio è vuoto";
    UpArchiveLacks => "the archive does not contain {0}", "l'archivio non contiene {0}";
    UpNewNoStart => "the new program does not start: {0}", "il programma nuovo non parte: {0}";
    UpNewExit => "the new program exited with {0}", "il programma nuovo è uscito con {0}";
    UpNewSlow => "the new program did not answer `--version` in time", "il programma nuovo non ha risposto a `--version` in tempo";
    UpNewWait => "cannot wait for the new program: {0}", "impossibile attendere il programma nuovo: {0}";
    UpCannotRunnable => "cannot make the new program runnable: {0}", "impossibile rendere eseguibile il programma nuovo: {0}";
    UpCannotPut => "cannot put the new program at {0}: {1}", "impossibile mettere il programma nuovo in {0}: {1}";
    UpCannotMove => "cannot move {0} out of the way: {1}", "impossibile spostare {0} per fare posto: {1}";
    UpCannotPutKept => "cannot put the new program at {0}: {1}; the old one is at {2} ({3})", "impossibile mettere il programma nuovo in {0}: {1}; il vecchio è in {2} ({3})";
    UpRestored => "an update was cut short and {0} was missing: the program that was there before has been put back. Start rada again.", "un aggiornamento si era interrotto e {0} mancava: il programma che c'era prima è stato rimesso al suo posto. Avvia di nuovo rada.";
    UpCannotKeepCopy => "cannot keep a copy of {0}: {1}", "impossibile tenere una copia di {0}: {1}";
    UpCannotKeepOld => "cannot keep the old program: {0}", "impossibile tenere il programma vecchio: {0}";
    UpNoEarlier => "there is no earlier version saved to go back to", "non c'è nessuna versione precedente salvata a cui tornare";
    UpSavedSays => "the saved program says \"{0}\", not its version", "il programma salvato dice \"{0}\", non la sua versione";
    UpCannotStart => "cannot start {0}: {1}", "impossibile avviare {0}: {1}";
    UpCannotWaitFor => "cannot wait for {0}: {1}", "impossibile attendere {0}: {1}";
    UpNoSourceDir => "the source folder {0} does not exist (see `source_dir` in the [update] section of config.toml)", "la cartella dei sorgenti {0} non esiste (vedi `source_dir` nella sezione [update] di config.toml)";
    UpCheckingCheckout => "checking the checkout", "controllo la copia git";
    UpNotCheckout => "{0} is not a git checkout ({1})", "{0} non è una copia git ({1})";
    UpDirtyOne => "the checkout in {0} has 1 changed file that is not committed: commit or stash it, then update again. Nothing was changed.", "la copia in {0} ha 1 file modificato non salvato: salvalo con un commit o mettilo da parte, poi aggiorna di nuovo. Non è stato cambiato nulla.";
    UpDirtyMany => "the checkout in {0} has {1} changed files that are not committed: commit or stash them, then update again. Nothing was changed.", "la copia in {0} ha {1} file modificati non salvati: salvali con un commit o mettili da parte, poi aggiorna di nuovo. Non è stato cambiato nulla.";
    UpWrongBranch => "the checkout in {0} is on the branch `{1}`, not `main`: switch it to main (`git switch main`), then update again. Nothing was changed.", "la copia in {0} è sul ramo `{1}`, non su `main`: portala su main (`git switch main`), poi aggiorna di nuovo. Non è stato cambiato nulla.";
    UpPulling => "pulling the latest changes", "scarico le ultime modifiche";
    UpPullFailed => "`git pull --ff-only` did not work (the checkout may have diverged from its upstream); the log has the details: {0}", "`git pull --ff-only` non ha funzionato (la copia potrebbe essersi staccata dall'originale); i dettagli sono nel registro: {0}";
    UpAlreadyLatest => "The checkout is already at the latest commit and the program was built from it; nothing to build.", "La copia è già all'ultimo commit e il programma è stato compilato da lì; non c'è nulla da compilare.";
    UpBuilding => "building (this takes a few minutes)", "compilo (ci vogliono alcuni minuti)";
    UpBuildingWhat => "building: {0} ({1} compiled)", "compilo: {0} ({1} compilati)";
    UpBuildFailed => "the build failed; the program that was running is untouched. The log has the details: {0}", "la compilazione non è riuscita; il programma che girava non è stato toccato. I dettagli sono nel registro: {0}";
    UpBuildLeftNone => "the build left no program at {0}", "la compilazione non ha lasciato nessun programma in {0}";
    UpSourceNewSays => "the new program says \"{0}\", not its version", "il programma nuovo dice \"{0}\", non la sua versione";
    UpPutting => "putting the new program in place", "metto a posto il programma nuovo";
    UpCannotWriteNext => "cannot write the new program next to {0}: {1}", "impossibile scrivere il programma nuovo accanto a {0}: {1}";
    UpBuiltFrom => "Built from {0} and installed at {1}.", "Compilata da {0} e installata in {1}.";
    UpBuildKept => " The build folder was kept because rada runs from it.", " La cartella di compilazione è stata tenuta perché rada gira da lì.";
    UpCleaning => "cleaning up the build folder", "pulisco la cartella di compilazione";
    UpHintBrew => "Homebrew installed it: run `brew upgrade rada`.", "L'ha installata Homebrew: esegui `brew upgrade rada`.";
    UpHintScoop => "Scoop installed it: run `scoop update rada`.", "L'ha installata Scoop: esegui `scoop update rada`.";
    UpHintStore => "It was installed from the Microsoft Store or winget: update it there.", "È stata installata dal Microsoft Store o da winget: aggiornala da lì.";
    UpHintChoco => "Chocolatey installed it: run `choco upgrade rada`.", "L'ha installata Chocolatey: esegui `choco upgrade rada`.";
    BarZoxide => "zoxide", "zoxide";
    UpHintCargo => "It was installed with cargo: run `{0}` to update it.", "È stata installata con cargo: esegui `{0}` per aggiornarla.";
    UpHintNix => "Nix installed it: update your profile or flake.", "L'ha installata Nix: aggiorna il tuo profilo o il tuo flake.";
    UpHintSnap => "It was installed as a snap or flatpak: update it with that tool.", "È stata installata come snap o flatpak: aggiornala con quello strumento.";
    UpHintSystem => "It is in {0}, which a package manager looks after: update it with {1}.", "Si trova in {0}, di cui si occupa un gestore di pacchetti: aggiornala con {1}.";
    UpHintDefault => "Update it with the tool that installed it.", "Aggiornala con lo strumento che l'ha installata.";
    UpToolArch => "an AUR helper (for example `yay -Syu`) or `sudo pacman -Syu`", "un helper per l'AUR (per esempio `yay -Syu`) o `sudo pacman -Syu`";
    UpToolPackageManager => "your package manager", "il tuo gestore di pacchetti";
    UpHttpNotFound => "{0} was not found (404)", "{0} non è stato trovato (404)";
    UpHttpRefused => "{0} refused the request ({1}); GitHub limits how often releases are read, try again later", "{0} ha rifiutato la richiesta ({1}); GitHub limita ogni quanto si leggono le versioni, riprova più tardi";
    UpHttpStatus => "{0} answered with status {1}", "{0} ha risposto con lo stato {1}";
    UpHttpUnreachable => "cannot reach {0}: {1}", "impossibile raggiungere {0}: {1}";
    UpHttpAnswer => "cannot read the answer of {0}: {1}", "impossibile leggere la risposta di {0}: {1}";
    UpHttpLarge => "{0} is larger than the {1} bytes allowed", "{0} è più grande dei {1} byte consentiti";
    UpHttpBroke => "the download of {0} broke off: {1}", "lo scaricamento di {0} si è interrotto: {1}";
    UpHttpEarly => "the download of {0} ended early ({1} of {2} bytes)", "lo scaricamento di {0} è finito troppo presto ({1} byte su {2})";
    UpNotReleases => "not a list of releases: {0}", "non è un elenco di versioni: {0}";

    // ---- rada as the system's file chooser (`--chooser-file`)
    ChooseTitleFile => "Choose a file", "Scegli un file";
    ChooseTitleFiles => "Choose files", "Scegli uno o più file";
    ChooseTitleFolder => "Choose a folder", "Scegli una cartella";
    ChooseTitleFolders => "Choose folders", "Scegli una o più cartelle";
    ChooseTitleSave => "Save as", "Salva come";
    ChooseHintFile => "Enter choose · → open folder · ← up · Esc cancel", "Invio scegli · → apri cartella · ← su · Esc annulla";
    ChooseHintFiles => "Space mark · Enter choose the marked · → open folder · ← up · Esc cancel", "Spazio segna · Invio scegli i segnati · → apri cartella · ← su · Esc annulla";
    ChooseHintFolder => "Enter choose the folder under the cursor · Alt+Enter the one you are in · → open · ← up · Esc cancel", "Invio scegli la cartella sotto il cursore · Alt+Invio quella in cui sei · → apri · ← su · Esc annulla";
    ChooseHintFolders => "Space mark · Enter choose the marked · Alt+Enter the folder you are in · → open · Esc cancel", "Spazio segna · Invio scegli i segnati · Alt+Invio la cartella in cui sei · → apri · Esc annulla";
    ChooseHintSave => "Ctrl+S or Alt+Enter save in this folder · Enter on a file: replace it · → open folder · ← up · Esc cancel", "Ctrl+S o Alt+Invio salva in questa cartella · Invio su un file: lo sostituisce · → apri cartella · ← su · Esc annulla";
    // The same, for a window too narrow for the long ones.
    ChooseHintFileShort => "Enter choose · Esc cancel", "Invio scegli · Esc annulla";
    ChooseHintFilesShort => "Space mark · Enter choose", "Spazio segna · Invio scegli";
    ChooseHintFolderShort => "Enter choose · Alt+Enter here", "Invio scegli · Alt+Invio qui";
    ChooseHintFoldersShort => "Space mark · Enter choose · Alt+Enter here", "Spazio segna · Invio scegli · Alt+Invio qui";
    ChooseHintSaveShort => "Ctrl+S save · Esc cancel", "Ctrl+S salva · Esc annulla";
    ChooseSaveName => "File name", "Nome del file";
    ChooseNameBad => "a name cannot contain / and cannot be . or ..", "un nome non può contenere / e non può essere . o ..";
    ChooseIsFolder => "{0} is a folder", "{0} è una cartella";
    ChooseExists => "{0} already exists: press Enter again to replace it", "{0} esiste già: premi di nuovo Invio per sostituirlo";
    ChooseLineBreak => "that name has a line break and cannot be handed over", "quel nome contiene un a capo e non si può consegnare";
    ChooseOnlyFiles => "only files can be chosen here", "qui si possono scegliere solo file";
    ChooseNothingMarked => "nothing marked: mark files with Space or press Enter on one", "niente di segnato: segna dei file con Spazio o premi Invio su uno";
    ChooseRefused => "not available while choosing", "non disponibile mentre si sceglie";
    ChooseNoParent => "the folder to save in is not there", "la cartella in cui salvare non c'è";

    // ---- `rada --chooser-file` and `rada setup file-picker`
    CliArgChooserFile => "Act as a file chooser: write the paths chosen to FILE, one per line, and quit. Enter chooses, Esc cancels and writes nothing. Nothing can be deleted, moved or renamed in this mode.", "Fa da finestra di scelta file: scrive in FILE i percorsi scelti, uno per riga, ed esce. Invio sceglie, Esc annulla e non scrive nulla. In questa modalità non si può eliminare, spostare né rinominare niente.";
    CliArgChooserMultiple => "With --chooser-file: more than one file (or folder) can be chosen.", "Con --chooser-file: si può scegliere più di un file (o più di una cartella).";
    CliArgChooserDir => "With --chooser-file: choose a folder instead of a file.", "Con --chooser-file: si sceglie una cartella invece di un file.";
    CliArgChooserSave => "With --chooser-file: choose where to save, with the name to suggest (`--chooser-save=NAME`; the folder is the path given).", "Con --chooser-file: si sceglie dove salvare, con il nome da suggerire (`--chooser-save=NOME`; la cartella è il percorso dato).";
    CliSetupFilePicker => "Make rada the system's file chooser (Linux, through xdg-desktop-portal-termfilechooser): shows what it would write and where, and writes only after you say yes. Copies of the files it changes are kept; nothing needs administrator rights and no package is installed.", "Fa di rada la finestra di scelta file del sistema (Linux, tramite xdg-desktop-portal-termfilechooser): mostra che cosa scriverebbe e dove, e scrive solo dopo il tuo sì. Tiene una copia dei file che cambia; non servono diritti di amministratore e non installa pacchetti.";
    CliArgRemoveFilePicker => "Go back: restore the files that were changed and take away the ones that were made.", "Torna indietro: ripristina i file cambiati e toglie quelli creati.";
    CliArgFilePickerYes => "Do not ask before writing.", "Non chiede conferma prima di scrivere.";
    // ---- `rada setup shell`
    CliSetupShell => "Make the shell follow the folder rada was left in (`v` instead of `rada`): writes the call of `rada --init` in the right place for your shell, after showing it and asking. Only a file of its own (fish, nushell) or a block between `# >>> rada >>>` and `# <<< rada <<<` is written; nothing needs administrator rights.", "Fa seguire alla shell la cartella in cui hai lasciato rada (`v` al posto di `rada`): scrive la chiamata a `rada --init` nel posto giusto per la tua shell, dopo avertela mostrata e aver chiesto conferma. Si scrive solo un file proprio (fish, nushell) o un blocco tra `# >>> rada >>>` e `# <<< rada <<<`; non servono diritti di amministratore.";
    CliArgShellName => "The shell: bash, zsh, fish, nushell or powershell (default: the one in $SHELL; PowerShell on Windows).", "La shell: bash, zsh, fish, nushell o powershell (predefinita: quella in $SHELL; PowerShell su Windows).";
    CliArgRemoveShell => "Take away exactly what a previous setup wrote: the file or the block, nothing else.", "Toglie esattamente ciò che una configurazione precedente ha scritto: il file o il blocco, nient'altro.";
    CliArgShellYes => "Do not ask before writing.", "Non chiede conferma prima di scrivere.";
    SsUnknownShell => "cannot tell which shell to set up (\"{0}\"); choose one with --shell bash|zsh|fish|nushell|powershell", "non so per quale shell configurare (\"{0}\"); scegline una con --shell bash|zsh|fish|nushell|powershell";
    SsIntro => "The {0} shell would follow the folder rada was left in. It would write:", "La shell {0} seguirebbe la cartella in cui hai lasciato rada. Scriverebbe:";
    SsReplace => "replace {0}", "sostituisce {0}";
    SsAdd => "add to {0}", "aggiunge a {0}";
    SsAlready => "already set up in {0}: nothing was written", "già configurato in {0}: non è stato scritto nulla";
    SsByHand => "{0} already calls rada --init ({1}): nothing was written", "{0} chiama già rada --init ({1}): non è stato scritto nulla";
    SsDone => "Use `{0}` instead of `rada`: it opens rada and, when you quit, the shell goes to the folder you were in. Open a new terminal first, so that the shell reads it.", "Usa `{0}` al posto di `rada`: apre rada e, quando esci, la shell va nella cartella in cui eri. Apri prima un nuovo terminale, così la shell lo legge.";
    SsNothingToRemove => "nothing to remove: rada has not set up this shell", "niente da togliere: rada non ha configurato questa shell";
    SsRemovedFile => "removed {0}", "tolto {0}";
    SsRemovedBlock => "took the rada block out of {0}", "tolto il blocco di rada da {0}";
    MChooserWithRequest => "--chooser-file cannot be used with --request or --demo", "--chooser-file non si può usare con --request o --demo";
    FpLinuxOnly => "the file chooser setup is for Linux (xdg-desktop-portal); nothing was done", "la configurazione della scelta file è per Linux (xdg-desktop-portal); non è stato fatto nulla";
    FpIntro => "rada will become the file chooser of this user's desktop. It would write:", "rada diventerà la finestra di scelta file del desktop di questo utente. Scriverebbe:";
    FpCreate => "create {0}", "crea {0}";
    FpChange => "change {0}  (a copy is kept as {1})", "cambia {0}  (una copia resta in {1})";
    FpUnchanged => "{0} is already as it should be", "{0} è già come deve essere";
    FpNoSudo => "Nothing needs administrator rights, and no package is installed.", "Non servono diritti di amministratore e non viene installato nessun pacchetto.";
    FpPortalMissing => "The portal itself is not installed (no termfilechooser.portal found). On Arch Linux install it from the AUR: yay -S xdg-desktop-portal-termfilechooser-hunkyburrito-git", "Il portale non è installato (nessun termfilechooser.portal trovato). Su Arch Linux installalo dall'AUR: yay -S xdg-desktop-portal-termfilechooser-hunkyburrito-git";
    FpAsk => "Write these files? [y/N] ", "Scrivere questi file? [s/N] ";
    FpAborted => "nothing was written", "non è stato scritto nulla";
    FpWrote => "wrote {0}", "scritto {0}";
    FpKeptCopy => "copy kept: {0}", "copia tenuta: {0}";
    FpNext => "Next: restart the portals with `systemctl --user restart xdg-desktop-portal`. In Firefox or Zen set widget.use-xdg-desktop-portal.file-picker to 1 in about:config.", "Poi: riavvia i portali con `systemctl --user restart xdg-desktop-portal`. In Firefox o Zen imposta widget.use-xdg-desktop-portal.file-picker a 1 in about:config.";
    FpRestored => "restored {0} from {1}", "ripristinato {0} da {1}";
    FpNothingToRemove => "nothing to remove: rada has not set up the file chooser", "niente da togliere: rada non ha configurato la scelta file";
    FpChangedSince => "{0} was changed after the setup: left as it is", "{0} è stato cambiato dopo la configurazione: lasciato com'è";
    FpAfterRemove => "Restart the portals with `systemctl --user restart xdg-desktop-portal` to finish.", "Riavvia i portali con `systemctl --user restart xdg-desktop-portal` per finire.";

    // ---- Windows: rada as the program that opens folders (`setup desktop --default-file-manager`)
    WfIntro => "rada would open the folders that other programs ask to open (the `open` of the Directory class). It would write, in your registry only (HKEY_CURRENT_USER; nothing for the whole machine):", "rada aprirebbe le cartelle che altri programmi chiedono di aprire (l'`open` della classe Directory). Scriverebbe, solo nel tuo registro (HKEY_CURRENT_USER; niente per tutta la macchina):";
    WfSetNew => "set  {0}\n       {1} = {2}   (new)", "scrive  {0}\n       {1} = {2}   (nuovo)";
    WfSetWas => "set  {0}\n       {1} = {2}   (was: {3})", "scrive  {0}\n       {1} = {2}   (era: {3})";
    WfDelete => "delete the value  {1}  of  {0}   (was: {2})", "toglie il valore  {1}  di  {0}   (era: {2})";
    WfProgram => "The program is {0}, and the folder is what it is given. Windows opens it in a console window (in Windows Terminal, if that is your default terminal).", "Il programma è {0}, e la cartella è quello che riceve. Windows lo apre in una finestra di console (in Windows Terminal, se è il tuo terminale predefinito).";
    WfExplorerStays => "Explorer is not touched: it still opens its own folders, Win+E and what programs ask of Explorer by name stay as they are, and *Open in new window* in a folder's right-click menu is still Explorer's.", "Explorer non viene toccato: apre ancora le sue cartelle, Win+E e quello che i programmi chiedono a Explorer per nome restano com'erano, e *Apri in una nuova finestra* nel menu del tasto destro di una cartella è ancora di Explorer.";
    WfAsk => "Write these to the registry? [y/N] ", "Scrivere questo nel registro? [s/N] ";
    WfDeclined => "the registry was not changed", "il registro non è stato cambiato";
    WfAlready => "the registry is already as it should be", "il registro è già come deve essere";
    WfDone => "rada now opens folders for other programs: {0}", "ora rada apre le cartelle per gli altri programmi: {0}";
    WfByHand => "To undo it: `rada setup desktop --remove`. If rada cannot run, by hand: reg delete \"{0}\" /f (and set the (Default) of Directory\\shell back if you had one).", "Per tornare indietro: `rada setup desktop --remove`. Se rada non parte, a mano: reg delete \"{0}\" /f (e rimetti il (Default) di Directory\\shell se ne avevi uno).";
    WfNoProgram => "cannot set it up: {0} is not there", "non si può configurare: {0} non c'è";
    WfRestored => "registry: put back {0}  {1} = {2}", "registro: rimesso {0}  {1} = {2}";
    WfValueRemoved => "registry: took away the value {1} of {0}", "registro: tolto il valore {1} di {0}";
    WfChangedSince => "registry: {0}  {1} was changed after the setup: left as it is", "registro: {0}  {1} è stato cambiato dopo la configurazione: lasciato com'è";
}
