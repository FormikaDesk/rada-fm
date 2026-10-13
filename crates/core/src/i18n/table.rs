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
    LClearSelection => "Clear selection / cancel", "Annulla la selezione / interrompi";
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
    LToggleView => "Switch between Details and Icons", "Passa tra Dettagli e Icone";
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
    ResultOne => "result", "risultato";
    ResultMany => "results", "risultati";
    ItemIsOne => "item is", "elemento è";
    ItemsAreMany => "items are", "elementi sono";

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
    PropsSelected => "{0} selected", "{0} selezionati";

    // ---- windows about the system, disks and updates
    ChipNoAccess => "No access", "Accesso negato";
    MacAsksOnce => "macOS asks you once, for each program, before it lets it into the Desktop, Documents, Downloads, iCloud Drive and external or network disks.", "macOS chiede il permesso una volta, per ogni programma, prima di lasciarlo entrare in Scrivania, Documenti, Download, iCloud Drive e nei dischi esterni o di rete.";
    BtnOpenFilesSettings => "Open Files & Folders settings", "Apri le impostazioni File e cartelle";
    BtnFullDisk => "Full Disk Access", "Accesso completo al disco";
    BtnClose => "Close", "Chiudi";
    BtnCancel => "Cancel", "Annulla";
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
    UpdateBinary => "rada will download the program for this system, check it against the release's SHA256SUMS and replace this one. The old one is kept: `rada update --rollback` brings it back.", "rada scaricherà il programma per questo sistema, lo verificherà con gli SHA256SUMS della versione e sostituirà questo. Quello vecchio viene conservato: `rada update --rollback` lo riporta.";
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
    HintCancel => "cancel", "annulla";
    HintMove => "move", "sposta";
    HintGo => "go", "vai";
    ChipProperties => "Properties", "Proprietà";
    ScanTotals => "{0} files · {1} folders · {2}", "{0} file · {1} cartelle · {2}";
    ChipError => "Error", "Errore";
    WentWrong => "Something went wrong", "Qualcosa è andato storto";
    AttemptN => "attempt {0}", "tentativo {0}";
    BtnSkip => "Skip", "Salta";
    BtnSkipAll => "Skip all", "Salta tutto";
    BtnRetry => "Retry", "Riprova";
    BtnAbort => "Abort", "Interrompi";
    AbortKeeps => "Abort keeps what is already done; u undoes it.", "Interrompi conserva ciò che è già fatto; u lo annulla.";
    ChipDone => "Done", "Fatto";
    BtnUndoDone => "Undo what was done", "Annulla ciò che è stato fatto";
    ChipRunning => "Running", "In corso";
    OpInProgress => "An operation is in progress", "Un'operazione è in corso";
    QuitWarn1 => "Quitting cancels it. Steps already finished stay in the", "Uscendo la annulli. I passaggi già conclusi restano nel";
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
    PolicyChange => "c to change", "c per cambiare";
    PolicyTrashNote => "replaced items go to the trash first, so undo brings them back", "gli elementi sostituiti vanno prima nel cestino, così annullando tornano";
    PlanRenames => "Renames", "Rinomine";
    PlanWhat => "What will happen", "Cosa succederà";
    PlanTabSteps => "Tab: every step", "Tab: ogni passaggio";
    PlanTabSummary => "Tab: summary", "Tab: riepilogo";
    PlanWhatN => "What will happen ({0})", "Cosa succederà ({0})";
    ActExtractItem => "extract", "estrai";
    ActAddItem => "add", "aggiungi";
    AsName => "as {0}", "come {0}";
    AndNMore => "… and {0} more", "… e altri {0}";
    MoreStepsNotListed => "… more steps not listed", "… altri passaggi non elencati";
    CannotBeUndone => "This cannot be undone.", "Non si può annullare.";
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
    HelpAlso2 => "Ctrl+1 / Ctrl+2 (or v) switch between Details and Icons; in Icons the arrows and h j k l move across the grid.", "Ctrl+1 / Ctrl+2 (o v) passano da Dettagli a Icone; nelle Icone le frecce e h j k l si muovono nella griglia.";
    HelpAlso3 => "Backspace goes to the parent folder with the vim keys on, and back with the classic keys alone.", "Backspace va alla cartella superiore con i tasti vim attivi, e indietro con i soli tasti classici.";
    HelpAlso4 => "Shift+Del deletes for good: the plan says so, and it cannot be undone.", "Maiusc+Canc elimina per sempre: il piano lo dice, e non si può annullare.";
    HelpEnd1 => "Enter on an archive opens it like a folder, read-only: copy items out (Ctrl+C), paste in a folder.", "Invio su un archivio lo apre come una cartella, in sola lettura: copia fuori gli elementi (Ctrl+C), incolla in una cartella.";
    HelpEnd2 => "Extract here / to a folder and Compress… need no archive tool installed; RAR needs 7z or unrar.", "Estrai qui / in una cartella e Comprimi… non richiedono programmi per archivi; per i RAR servono 7z o unrar.";
    HelpEnd3 => "Ctrl+C copies; it never quits. Quit with {0}.", "Ctrl+C copia; non chiude mai il programma. Esci con {0}.";
    HelpEnd4 => "Every operation shows a plan first. Every finished operation can be undone.", "Ogni operazione mostra prima un piano. Ogni operazione conclusa si può annullare.";
    HelpEnd5 => "Rebind keys in the [keys] section of the configuration file.", "Cambia i tasti nella sezione [keys] del file di configurazione.";
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
    MenuCheckUpdates => "Check for updates", "Cerca aggiornamenti";
    MenuAbout => "About rada", "Informazioni su rada";
    MenuNew => "New", "Nuovo";
    MenuSortBy => "Sort by", "Ordina per";
    MenuView => "View", "Vista";
    MenuTags => "Tags", "Tag";
    MenuNewTag => "New tag…", "Nuovo tag…";
    MenuRemoveAllTags => "Remove all tags", "Togli tutti i tag";
    WhatChangeTags => "change tags", "cambiare i tag";
    PlanningTags => "Planning", "Preparo il piano";
}
