//! UI localization. The English text is the key: `tr("Refresh")` returns the text in the
//! current language, falling back to English when a translation is missing. Strings with
//! values use named placeholders: `trf("{n} selected", &[("n", "3")])`.
//!
//! Only user-visible UI text goes through here; logs, MCP tool descriptions sent to agents
//! and backend error messages stay English.

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    PtBr,
    Es,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::En, Lang::PtBr, Lang::Es];

    /// Value stored in the config file.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::PtBr => "pt-BR",
            Lang::Es => "es",
        }
    }

    /// Name of the language in that language.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::PtBr => "Português (Brasil)",
            Lang::Es => "Español",
        }
    }

    fn from_u8(v: u8) -> Lang {
        match v {
            1 => Lang::PtBr,
            2 => Lang::Es,
            _ => Lang::En,
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Lang::En => 0,
            Lang::PtBr => 1,
            Lang::Es => 2,
        }
    }
}

static LANG: AtomicU8 = AtomicU8::new(0);

pub fn set_lang(lang: Lang) {
    LANG.store(lang.as_u8(), Ordering::Relaxed);
}

pub fn lang() -> Lang {
    Lang::from_u8(LANG.load(Ordering::Relaxed))
}

/// Language of the operating system (English when unknown or unsupported).
pub fn detect() -> Lang {
    from_locale(sys_locale::get_locale().as_deref())
}

fn from_locale(locale: Option<&str>) -> Lang {
    let l = locale.unwrap_or("").to_ascii_lowercase();
    if l.starts_with("pt") {
        Lang::PtBr
    } else if l.starts_with("es") {
        Lang::Es
    } else {
        Lang::En
    }
}

/// Turn the `interface.language` setting into a language: "auto" follows the OS.
pub fn resolve(setting: &str) -> Lang {
    match setting {
        "auto" => detect(),
        s => Lang::ALL.into_iter().find(|l| l.code() == s).unwrap_or(Lang::En),
    }
}

/// Translate an English UI string into the current language.
pub fn tr(en: &'static str) -> &'static str {
    tr_in(lang(), en)
}

fn tr_in(lang: Lang, en: &'static str) -> &'static str {
    if lang == Lang::En {
        return en;
    }
    match INDEX.get(en) {
        Some((pt, es)) => match lang {
            Lang::PtBr => pt,
            Lang::Es => es,
            Lang::En => en,
        },
        None => en,
    }
}

/// Translate and fill named placeholders: `trf("{n} items", &[("n", "4")])`.
pub fn trf(key: &'static str, args: &[(&str, &str)]) -> String {
    fill(tr(key), args)
}

fn fill(text: &str, args: &[(&str, &str)]) -> String {
    let mut out = text.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// Short month name (0 = January) for the current language; `None` means use chrono's English `%b`.
pub fn month_abbr(month0: u32) -> Option<&'static str> {
    const PT: [&str; 12] = ["jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez"];
    const ES: [&str; 12] = ["ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sep", "oct", "nov", "dic"];
    let i = month0 as usize % 12;
    match lang() {
        Lang::En => None,
        Lang::PtBr => Some(PT[i]),
        Lang::Es => Some(ES[i]),
    }
}

static INDEX: LazyLock<HashMap<&'static str, (&'static str, &'static str)>> =
    LazyLock::new(|| STRINGS.iter().map(|(en, pt, es)| (*en, (*pt, *es))).collect());

/// (English key, Português (Brasil), Español)
#[rustfmt::skip]
static STRINGS: &[(&str, &str, &str)] = &[
    ("Shortcuts", "Atalhos", "Atajos"),
    ("Back (previous folder)", "Voltar (pasta anterior)", "Atrás (carpeta anterior)"),
    ("Forward (next folder)", "Avançar (próxima pasta)", "Adelante (carpeta siguiente)"),
    ("Parent folder", "Pasta acima", "Carpeta superior"),
    ("Open folder / transfer selection", "Abrir pasta / transferir seleção", "Abrir carpeta / transferir selección"),
    ("Type a path", "Digitar um caminho", "Escribir una ruta"),
    ("Select all", "Selecionar tudo", "Seleccionar todo"),
    ("Process queue", "Processar fila", "Procesar cola"),
    ("Show / hide the transfer panel", "Mostrar / ocultar o painel de transferências", "Mostrar / ocultar el panel de transferencias"),
    ("Space", "Espaço", "Espacio"),
    ("Mouse back button", "Botão voltar do mouse", "Botón atrás del ratón"),
    ("Mouse forward button", "Botão avançar do mouse", "Botón adelante del ratón"),
    ("Middle click", "Clique do meio", "Clic central"),
    ("{key} was removed from \"{action}\"", "{key} foi removido de \"{action}\"", "{key} se quitó de \"{action}\""),
    ("Press a mouse button… (side buttons or middle)", "Pressione um botão do mouse… (laterais ou do meio)", "Pulsa un botón del ratón… (laterales o central)"),
    ("Press a key… (Esc to cancel)", "Pressione uma tecla… (Esc cancela)", "Pulsa una tecla… (Esc cancela)"),
    ("Remove", "Remover", "Quitar"),
    ("Click a cell, then press the key or mouse button to use. Mouse buttons act on the pane under the pointer.", "Clique numa célula e pressione a tecla ou o botão do mouse desejado. Os botões do mouse agem no painel sob o ponteiro.", "Haz clic en una celda y pulsa la tecla o el botón del ratón que quieras. Los botones del ratón actúan en el panel bajo el puntero."),
    ("Reset to defaults", "Restaurar padrões", "Restablecer valores predeterminados"),
    ("ACTION", "AÇÃO", "ACCIÓN"),
    ("KEYBOARD", "TECLADO", "TECLADO"),
    ("MOUSE", "MOUSE", "RATÓN"),
    ("Transfer it to the other side, or open it in your editor like View / Edit (remote files are uploaded back when you save).", "Transfere para o outro lado, ou abre no seu editor como o Ver / Editar (arquivos remotos voltam para o servidor quando você salva).", "Lo transfiere al otro lado, o lo abre en tu editor como Ver / Editar (los archivos remotos se suben de nuevo al guardar)."),
    ("Upload edited files automatically", "Enviar arquivos editados automaticamente", "Subir archivos editados automáticamente"),
    ("With View / Edit, every save is uploaded right away and replaces the file on the server, without asking.", "No Ver / Editar, cada vez que você salvar o arquivo é enviado na hora e substitui o do servidor, sem perguntar.", "Con Ver / Editar, cada vez que guardes el archivo se sube al momento y reemplaza el del servidor, sin preguntar."),
    ("Saved {name}: uploading it to the server", "{name} salvo: enviando para o servidor", "{name} guardado: subiéndolo al servidor"),
    ("SmartFTP is running as administrator, so Windows blocks dragging files in from Explorer. Close it and open it again from the Start menu to fix this.", "O SmartFTP está rodando como administrador, então o Windows bloqueia arrastar arquivos do Explorer. Feche e abra de novo pelo menu Iniciar para resolver.", "SmartFTP se está ejecutando como administrador, así que Windows bloquea arrastrar archivos desde el Explorador. Ciérralo y ábrelo de nuevo desde el menú Inicio para solucionarlo."),
    ("Dismiss", "Dispensar", "Descartar"),
    ("Open for editing", "Abrir para edição", "Abrir para editar"),
    // queue and view / edit
    ("Add to queue", "Adicionar à fila", "Añadir a la cola"),
    ("Queue {n}", "Fila {n}", "Cola {n}"),
    ("Clear queue", "Limpar fila", "Vaciar cola"),
    ("Process queue ({n})", "Processar fila ({n})", "Procesar cola ({n})"),
    ("The queue is empty. Right-click files and choose Add to queue, then Process queue to start them.", "A fila está vazia. Clique com o botão direito nos arquivos, escolha Adicionar à fila e depois Processar fila para iniciá-los.", "La cola está vacía. Haz clic derecho en los archivos, elige Añadir a la cola y luego Procesar cola para iniciarlos."),
    ("in queue", "na fila", "en cola"),
    ("Start now", "Iniciar agora", "Iniciar ahora"),
    ("Remove from queue", "Remover da fila", "Quitar de la cola"),
    ("Added {n} item(s) to the queue", "{n} item(ns) adicionado(s) à fila", "{n} elemento(s) añadido(s) a la cola"),
    ("Start transfers waiting in the queue", "Inicia as transferências que estão na fila", "Inicia las transferencias que esperan en la cola"),
    ("View / Edit", "Ver / Editar", "Ver / Editar"),
    ("Editing {n}", "Editando {n}", "Editando {n}"),
    ("Opening {name}…", "Abrindo {name}…", "Abriendo {name}…"),
    ("Could not open the file: {error}", "Não foi possível abrir o arquivo: {error}", "No se pudo abrir el archivo: {error}"),
    ("\"{name}\" was changed.", "\"{name}\" foi alterado.", "\"{name}\" se modificó."),
    ("Upload it to {site} and replace {path}?", "Enviar para {site} e substituir {path}?", "¿Subirlo a {site} y reemplazar {path}?"),
    ("Upload and replace", "Enviar e substituir", "Subir y reemplazar"),
    ("Not now", "Agora não", "Ahora no"),
    ("Stop editing", "Parar de editar", "Dejar de editar"),
    ("The session for this file was closed.", "A sessão deste arquivo foi fechada.", "La sesión de este archivo se cerró."),
    ("Uploading {name}…", "Enviando {name}…", "Subiendo {name}…"),
    ("No files are being edited. Right-click a remote file and choose View / Edit (F4).", "Nenhum arquivo em edição. Clique com o botão direito num arquivo remoto e escolha Ver / Editar (F4).", "No se está editando ningún archivo. Haz clic derecho en un archivo remoto y elige Ver / Editar (F4)."),
    ("uploaded {n}×", "enviado {n}×", "subido {n}×"),
    ("Upload now", "Enviar agora", "Subir ahora"),
    ("File editor", "Editor de arquivos", "Editor de archivos"),
    ("Open files with", "Abrir arquivos com", "Abrir archivos con"),
    ("Used by View / Edit (F4). Remote files are downloaded to a temporary folder; when you save, SmartFTP asks whether to upload the file and replace the one on the server.", "Usado por Ver / Editar (F4). Arquivos remotos são baixados para uma pasta temporária; quando você salva, o SmartFTP pergunta se deve enviar o arquivo e substituir o do servidor.", "Lo usa Ver / Editar (F4). Los archivos remotos se descargan a una carpeta temporal; al guardar, SmartFTP pregunta si debe subir el archivo y reemplazar el del servidor."),
    ("System default", "Padrão do sistema", "Predeterminado del sistema"),
    ("Choose program…", "Escolher programa…", "Elegir programa…"),
    ("Choose editor", "Escolher editor", "Elegir editor"),
    ("Command line. {file} is replaced by the file path; leave empty to use the system default app.", "Linha de comando. {file} é substituído pelo caminho do arquivo; deixe vazio para usar o app padrão do sistema.", "Línea de comandos. {file} se reemplaza por la ruta del archivo; déjalo vacío para usar la app predeterminada del sistema."),
    // updates
    ("SmartFTP {version} is available.", "SmartFTP {version} está disponível.", "SmartFTP {version} está disponible."),
    ("SmartFTP is up to date.", "O SmartFTP está atualizado.", "SmartFTP está actualizado."),
    ("Could not check for updates", "Não foi possível verificar atualizações", "No se pudo buscar actualizaciones"),
    ("Transfers in progress will be interrupted.", "As transferências em andamento serão interrompidas.", "Las transferencias en curso se interrumpirán."),
    ("SmartFTP restarts when the update is installed.", "O SmartFTP reinicia quando a atualização for instalada.", "SmartFTP se reinicia al instalar la actualización."),
    ("Install SmartFTP {version}?", "Instalar o SmartFTP {version}?", "¿Instalar SmartFTP {version}?"),
    ("Install and restart", "Instalar e reiniciar", "Instalar y reiniciar"),
    ("Later", "Depois", "Más tarde"),
    ("Updated. Restart SmartFTP to use the new version", "Atualizado. Reinicie o SmartFTP para usar a nova versão", "Actualizado. Reinicia SmartFTP para usar la nueva versión"),
    ("Update failed", "A atualização falhou", "La actualización falló"),
    ("Update to {version}", "Atualizar para {version}", "Actualizar a {version}"),
    ("Updating… {pct}%", "Atualizando… {pct}%", "Actualizando… {pct}%"),
    ("Checking…", "Verificando…", "Comprobando…"),
    ("Up to date", "Atualizado", "Actualizado"),
    ("Version {version} is available", "A versão {version} está disponível", "La versión {version} está disponible"),
    ("Downloading the update…", "Baixando a atualização…", "Descargando la actualización…"),
    ("Updates", "Atualizações", "Actualizaciones"),
    ("Version {version}", "Versão {version}", "Versión {version}"),
    ("Check now", "Verificar agora", "Comprobar ahora"),
    ("Check for updates on startup", "Verificar atualizações ao iniciar", "Buscar actualizaciones al iniciar"),
    ("Look for a newer release on GitHub when SmartFTP starts. Nothing is installed without asking.", "Procura uma versão mais nova no GitHub ao abrir o SmartFTP. Nada é instalado sem perguntar.", "Busca una versión más reciente en GitHub al abrir SmartFTP. No se instala nada sin preguntar."),
    ("Updates are downloaded from GitHub Releases and verified with SHA-256 before installing.", "As atualizações vêm do GitHub Releases e são verificadas com SHA-256 antes de instalar.", "Las actualizaciones se descargan de GitHub Releases y se verifican con SHA-256 antes de instalarlas."),
    // ---------------------------------------------------------------- workspace / toasts
    ("→ remote", "→ remoto", "→ remoto"),
    ("→ local", "→ local", "→ local"),
    ("{agent} opened a session to {site}", "{agent} abriu uma sessão em {site}", "{agent} abrió una sesión en {site}"),
    ("{name} failed: {error}", "{name} falhou: {error}", "{name} falló: {error}"),
    ("{path} is not a folder", "{path} não é uma pasta", "{path} no es una carpeta"),
    ("Could not connect to {site}: {error}", "Não foi possível conectar a {site}: {error}", "No se pudo conectar a {site}: {error}"),
    ("Use a URL like sftp://user:password@host:22/path", "Use uma URL como sftp://usuario:senha@host:22/caminho", "Usa una URL como sftp://usuario:contraseña@host:22/ruta"),
    ("Connect to a site first", "Conecte-se a um site primeiro", "Conéctate primero a un sitio"),
    ("Upload failed: {error}", "Falha no envio: {error}", "Error al subir: {error}"),
    ("Download failed: {error}", "Falha no download: {error}", "Error al descargar: {error}"),
    ("Move failed: {error}", "Falha ao mover: {error}", "Error al mover: {error}"),
    ("Delete \"{name}\" from this computer?", "Excluir \"{name}\" deste computador?", "¿Eliminar \"{name}\" de este equipo?"),
    ("Delete \"{name}\" from {site}?", "Excluir \"{name}\" de {site}?", "¿Eliminar \"{name}\" de {site}?"),
    ("Delete {n} items from this computer?", "Excluir {n} itens deste computador?", "¿Eliminar {n} elementos de este equipo?"),
    ("Delete {n} items from {site}?", "Excluir {n} itens de {site}?", "¿Eliminar {n} elementos de {site}?"),
    ("Folders are deleted with everything inside. This cannot be undone.", "As pastas são excluídas com todo o conteúdo. Isso não pode ser desfeito.", "Las carpetas se eliminan con todo su contenido. No se puede deshacer."),
    ("Folder name", "Nome da pasta", "Nombre de la carpeta"),
    ("File name", "Nome do arquivo", "Nombre del archivo"),
    ("New name", "Novo nome", "Nuevo nombre"),
    ("Octal mode, e.g. 755", "Modo octal, ex.: 755", "Modo octal, p. ej. 755"),
    ("mode must be octal, e.g. 755", "o modo deve ser octal, ex.: 755", "el modo debe ser octal, p. ej. 755"),
    ("Folder on this computer", "Pasta neste computador", "Carpeta en este equipo"),
    ("Folder not found: {path}", "Pasta não encontrada: {path}", "Carpeta no encontrada: {path}"),
    ("Open folder", "Abrir pasta", "Abrir carpeta"),
    ("Path copied", "Caminho copiado", "Ruta copiada"),
    // ---------------------------------------------------------------- context menus
    ("Open", "Abrir", "Abrir"),
    ("Upload", "Enviar", "Subir"),
    ("Download", "Baixar", "Descargar"),
    ("Rename", "Renomear", "Renombrar"),
    ("Permissions…", "Permissões…", "Permisos…"),
    ("Copy path", "Copiar caminho", "Copiar ruta"),
    ("Show in file manager", "Mostrar no gerenciador de arquivos", "Mostrar en el explorador de archivos"),
    ("Delete", "Excluir", "Eliminar"),
    ("New folder", "Nova pasta", "Nueva carpeta"),
    ("New file", "Novo arquivo", "Nuevo archivo"),
    ("Refresh", "Atualizar", "Actualizar"),
    ("Connect", "Conectar", "Conectar"),
    ("Edit site…", "Editar site…", "Editar sitio…"),
    ("Icon & color…", "Ícone e cor…", "Icono y color…"),
    ("Remove from folder", "Remover da pasta", "Quitar de la carpeta"),
    ("Disconnect", "Desconectar", "Desconectar"),
    ("Delete site", "Excluir site", "Eliminar sitio"),
    ("Save as site…", "Salvar como site…", "Guardar como sitio…"),
    ("Collapse", "Recolher", "Contraer"),
    ("Expand", "Expandir", "Expandir"),
    ("Rename folder…", "Renomear pasta…", "Renombrar carpeta…"),
    ("Ungroup", "Desagrupar", "Desagrupar"),
    ("Delete the site \"{name}\"?", "Excluir o site \"{name}\"?", "¿Eliminar el sitio \"{name}\"?"),
    ("Only the saved connection is removed; no files are touched.", "Só a conexão salva é removida; nenhum arquivo é alterado.", "Solo se elimina la conexión guardada; no se toca ningún archivo."),
    // ---------------------------------------------------------------- rail
    ("{name} · 1 site", "{name} · 1 site", "{name} · 1 sitio"),
    ("{name} · {n} sites", "{name} · {n} sites", "{name} · {n} sitios"),
    ("New site", "Novo site", "Nuevo sitio"),
    ("Quick connect (URL)", "Conexão rápida (URL)", "Conexión rápida (URL)"),
    ("{name} (not saved)", "{name} (não salvo)", "{name} (sin guardar)"),
    ("MCP & Agents", "MCP e agentes", "MCP y agentes"),
    ("Settings", "Configurações", "Configuración"),
    // ---------------------------------------------------------------- panes
    ("Not connected", "Não conectado", "Sin conexión"),
    ("Pick a site on the left, paste a URL into quick connect, or let an agent connect over MCP.", "Escolha um site à esquerda, cole uma URL na conexão rápida ou deixe um agente conectar via MCP.", "Elige un sitio a la izquierda, pega una URL en la conexión rápida o deja que un agente se conecte por MCP."),
    ("opened by {agent}", "aberta por {agent}", "abierta por {agent}"),
    ("Edit site", "Editar site", "Editar sitio"),
    ("No remote connected", "Nenhum servidor conectado", "Ningún servidor conectado"),
    ("Connect to a site to see its files here. Then click → / ← on any row, double-click a file, or drag files between the two sides.", "Conecte-se a um site para ver os arquivos aqui. Depois clique em → / ← em qualquer linha, dê um duplo clique em um arquivo ou arraste arquivos entre os dois lados.", "Conéctate a un sitio para ver sus archivos aquí. Luego haz clic en → / ← en cualquier fila, haz doble clic en un archivo o arrastra archivos entre los dos lados."),
    ("This computer", "Este computador", "Este equipo"),
    ("Click to type a path (Ctrl+L)", "Clique para digitar um caminho (Ctrl+L)", "Haz clic para escribir una ruta (Ctrl+L)"),
    ("Choose a folder…", "Escolher uma pasta…", "Elegir una carpeta…"),
    ("Parent folder (Backspace)", "Pasta acima (Backspace)", "Carpeta superior (Retroceso)"),
    ("Refresh (F5)", "Atualizar (F5)", "Actualizar (F5)"),
    ("New folder (F7)", "Nova pasta (F7)", "Nueva carpeta (F7)"),
    ("Show / hide dotfiles", "Mostrar / ocultar arquivos ocultos", "Mostrar / ocultar archivos ocultos"),
    ("Name", "Nome", "Nombre"),
    ("Size", "Tamanho", "Tamaño"),
    ("Modified", "Modificado", "Modificado"),
    ("Perms", "Perm.", "Perm."),
    ("Loading…", "Carregando…", "Cargando…"),
    ("This folder is empty", "Esta pasta está vazia", "Esta carpeta está vacía"),
    ("only here", "só aqui", "solo aquí"),
    ("▲ newer", "▲ mais novo", "▲ más reciente"),
    ("older", "mais antigo", "más antiguo"),
    ("{n} items", "{n} itens", "{n} elementos"),
    ("Upload to the remote folder", "Enviar para a pasta remota", "Subir a la carpeta remota"),
    ("Download to the local folder", "Baixar para a pasta local", "Descargar en la carpeta local"),
    ("Upload to {path}", "Enviar para {path}", "Subir a {path}"),
    ("Download to {path}", "Baixar para {path}", "Descargar en {path}"),
    ("{n} selected", "{n} selecionados", "{n} seleccionados"),
    ("Clear", "Limpar", "Limpiar"),
    ("Click → / ← on a row, double-click, or drag between panes", "Clique em → / ← em uma linha, dê um duplo clique ou arraste entre os painéis", "Haz clic en → / ← en una fila, doble clic o arrastra entre los paneles"),
    // ---------------------------------------------------------------- transfer drawer
    ("Transfers", "Transferências", "Transferencias"),
    ("Active {n}", "Ativas {n}", "Activas {n}"),
    ("Done {n}", "Concluídas {n}", "Completadas {n}"),
    ("Failed {n}", "Com falha {n}", "Fallidas {n}"),
    ("Activity", "Atividade", "Actividad"),
    ("Clear finished", "Limpar concluídas", "Limpiar finalizadas"),
    ("Nothing yet", "Nada ainda", "Nada todavía"),
    ("you", "você", "tú"),
    ("ok", "ok", "ok"),
    ("error", "erro", "error"),
    ("No active transfers. Select files and press → or ←, or drag them across.", "Nenhuma transferência ativa. Selecione arquivos e clique em → ou ←, ou arraste-os para o outro lado.", "No hay transferencias activas. Selecciona archivos y pulsa → o ←, o arrástralos al otro lado."),
    ("Finished transfers show up here.", "As transferências concluídas aparecem aqui.", "Las transferencias finalizadas aparecen aquí."),
    ("No failures.", "Nenhuma falha.", "Sin errores."),
    ("queued", "na fila", "en cola"),
    ("cancelled", "cancelada", "cancelada"),
    ("skipped (already exists)", "ignorada (já existe)", "omitida (ya existe)"),
    // ---------------------------------------------------------------- dialogs
    ("My server", "Meu servidor", "Mi servidor"),
    ("e.g. Production", "ex.: Produção", "p. ej. Producción"),
    ("username", "usuário", "usuario"),
    ("Saved — leave empty to keep", "Salva — deixe vazio para manter", "Guardada — déjala vacía para conservarla"),
    ("Password or key passphrase", "Senha ou frase-senha da chave", "Contraseña o frase de la clave"),
    ("~/.ssh/id_ed25519 (optional)", "~/.ssh/id_ed25519 (opcional)", "~/.ssh/id_ed25519 (opcional)"),
    ("/ (server default)", "/ (padrão do servidor)", "/ (predeterminada del servidor)"),
    ("local start folder (optional)", "pasta local inicial (opcional)", "carpeta local inicial (opcional)"),
    ("Port must be a number", "A porta deve ser um número", "El puerto debe ser un número"),
    ("Host is required", "O host é obrigatório", "El host es obligatorio"),
    ("Choose private key", "Escolher chave privada", "Elegir clave privada"),
    ("Sites", "Sites", "Sitios"),
    ("New", "Novo", "Nuevo"),
    ("FTPS (explicit TLS)", "FTPS (TLS explícito)", "FTPS (TLS explícito)"),
    ("Icon & color", "Ícone e cor", "Icono y color"),
    ("Group", "Grupo", "Grupo"),
    ("Host", "Host", "Host"),
    ("Port", "Porta", "Puerto"),
    ("Username", "Usuário", "Usuario"),
    ("Password", "Senha", "Contraseña"),
    ("Private key", "Chave privada", "Clave privada"),
    ("Browse", "Procurar", "Examinar"),
    ("Remote start folder", "Pasta remota inicial", "Carpeta remota inicial"),
    ("Local start folder", "Pasta local inicial", "Carpeta local inicial"),
    ("Allow AI agents (MCP)", "Permitir agentes de IA (MCP)", "Permitir agentes de IA (MCP)"),
    ("Agents can open this site and run file operations. Passwords are never exposed to them.", "Agentes podem abrir este site e realizar operações com arquivos. As senhas nunca são expostas a eles.", "Los agentes pueden abrir este sitio y realizar operaciones con archivos. Nunca ven las contraseñas."),
    ("Save", "Salvar", "Guardar"),
    ("Save & connect", "Salvar e conectar", "Guardar y conectar"),
    ("Create", "Criar", "Crear"),
    ("New empty file", "Novo arquivo vazio", "Nuevo archivo vacío"),
    ("Permissions", "Permissões", "Permisos"),
    ("Apply", "Aplicar", "Aplicar"),
    ("Quick connect", "Conexão rápida", "Conexión rápida"),
    ("Rename folder", "Renomear pasta", "Renombrar carpeta"),
    ("sftp://, ftp:// or ftps:// — the connection is not saved", "sftp://, ftp:// ou ftps:// — a conexão não é salva", "sftp://, ftp:// o ftps:// — la conexión no se guarda"),
    ("Folder in the connection rail", "Pasta na barra de conexões", "Carpeta en la barra de conexiones"),
    ("{name} on this computer", "{name} neste computador", "{name} en este equipo"),
    ("{name} on {site}", "{name} em {site}", "{name} en {site}"),
    ("in {path} on this computer", "em {path} neste computador", "en {path} en este equipo"),
    ("in {path} on {site}", "em {path} em {site}", "en {path} en {site}"),
    ("755 = rwxr-xr-x (folders, scripts) · 644 = rw-r--r-- (files) · 600 = owner only", "755 = rwxr-xr-x (pastas, scripts) · 644 = rw-r--r-- (arquivos) · 600 = só o dono", "755 = rwxr-xr-x (carpetas, scripts) · 644 = rw-r--r-- (archivos) · 600 = solo el propietario"),
    ("Cancel", "Cancelar", "Cancelar"),
    ("Customize icon and color", "Personalizar ícone e cor", "Personalizar icono y color"),
    ("ICON", "ÍCONE", "ICONO"),
    ("COLOR", "COR", "COLOR"),
    ("Saved with the site", "Salvo com o site", "Se guarda con el sitio"),
    ("Done", "Concluído", "Listo"),
    // ---------------------------------------------------------------- settings: appearance & MCP
    ("Theme error: {error}", "Erro no tema: {error}", "Error en el tema: {error}"),
    ("Created {path} — edit it and press Reload", "{path} criado — edite e clique em Recarregar", "Se creó {path} — edítalo y pulsa Recargar"),
    ("Could not write theme: {error}", "Não foi possível gravar o tema: {error}", "No se pudo guardar el tema: {error}"),
    ("General", "Geral", "General"),
    ("Connection", "Conexão", "Conexión"),
    ("Appearance", "Aparência", "Apariencia"),
    ("System", "Sistema", "Sistema"),
    ("Light", "Claro", "Claro"),
    ("Dark", "Escuro", "Oscuro"),
    ("Built-in", "Integrado", "Integrado"),
    ("Custom", "Personalizado", "Personalizado"),
    ("Pick a theme, or drop your own .toml file into the themes folder.", "Escolha um tema ou coloque seu próprio arquivo .toml na pasta de temas.", "Elige un tema o coloca tu propio archivo .toml en la carpeta de temas."),
    ("Mode", "Modo", "Modo"),
    ("System follows the OS and switches between your selected light and dark theme.", "Sistema segue o SO e alterna entre os temas claro e escuro selecionados.", "Sistema sigue al SO y alterna entre los temas claro y oscuro seleccionados."),
    ("Make your own", "Crie o seu", "Crea el tuyo"),
    ("A theme is a small TOML file with 16 colors. Duplicate the current one, edit the hex values, then press Reload.", "Um tema é um pequeno arquivo TOML com 16 cores. Duplique o atual, edite os valores hexadecimais e clique em Recarregar.", "Un tema es un pequeño archivo TOML con 16 colores. Duplica el actual, edita los valores hexadecimales y pulsa Recargar."),
    ("Duplicate current", "Duplicar atual", "Duplicar actual"),
    ("Reload", "Recarregar", "Recargar"),
    ("Copied to clipboard", "Copiado para a área de transferência", "Copiado al portapapeles"),
    ("MCP server", "Servidor MCP", "Servidor MCP"),
    ("Let AI agents drive SmartFTP through the Model Context Protocol. Agents see the same sessions you do.", "Deixe agentes de IA controlarem o SmartFTP pelo Model Context Protocol. Os agentes veem as mesmas sessões que você.", "Deja que agentes de IA controlen SmartFTP mediante el Model Context Protocol. Los agentes ven las mismas sesiones que tú."),
    ("Running", "Ativo", "Activo"),
    ("Error", "Erro", "Error"),
    ("Off", "Desligado", "Desactivado"),
    ("Connect an agent", "Conectar um agente", "Conectar un agente"),
    ("Endpoint (Streamable HTTP)", "Endpoint (Streamable HTTP)", "Endpoint (Streamable HTTP)"),
    ("Bearer token", "Token Bearer", "Token Bearer"),
    ("Token copied", "Token copiado", "Token copiado"),
    ("Regenerate", "Gerar novo", "Regenerar"),
    ("New token generated — update your agents", "Novo token gerado — atualize seus agentes", "Nuevo token generado — actualiza tus agentes"),
    ("Other MCP clients (JSON)", "Outros clientes MCP (JSON)", "Otros clientes MCP (JSON)"),
    ("Headless alternative: run `smartftp --mcp-stdio` as a stdio server (uses the same saved sites, but its own sessions).", "Alternativa sem interface: execute `smartftp --mcp-stdio` como servidor stdio (usa os mesmos sites salvos, mas sessões próprias).", "Alternativa sin interfaz: ejecuta `smartftp --mcp-stdio` como servidor stdio (usa los mismos sitios guardados, pero sus propias sesiones)."),
    ("Exposed tools", "Ferramentas expostas", "Herramientas expuestas"),
    ("Recent agent activity", "Atividade recente de agentes", "Actividad reciente de agentes"),
    ("No agent activity yet.", "Nenhuma atividade de agentes ainda.", "Aún no hay actividad de agentes."),
    // MCP tool descriptions (shown in Settings; the MCP server itself sends English)
    ("Saved connection profiles", "Perfis de conexão salvos", "Perfiles de conexión guardados"),
    ("Currently open sessions", "Sessões abertas no momento", "Sesiones abiertas actualmente"),
    ("Open a session to a saved site or URL", "Abrir uma sessão em um site salvo ou URL", "Abrir una sesión en un sitio guardado o URL"),
    ("Close an open session", "Fechar uma sessão aberta", "Cerrar una sesión abierta"),
    ("List a remote directory", "Listar um diretório remoto", "Listar un directorio remoto"),
    ("Change the session's working directory", "Mudar o diretório de trabalho da sessão", "Cambiar el directorio de trabajo de la sesión"),
    ("Create a remote directory", "Criar um diretório remoto", "Crear un directorio remoto"),
    ("Delete remote files or folders", "Excluir arquivos ou pastas remotos", "Eliminar archivos o carpetas remotos"),
    ("Rename / move a remote entry", "Renomear / mover um item remoto", "Renombrar / mover un elemento remoto"),
    ("Change remote permissions", "Alterar permissões remotas", "Cambiar permisos remotos"),
    ("Read a remote text file", "Ler um arquivo de texto remoto", "Leer un archivo de texto remoto"),
    ("Create or overwrite a remote file", "Criar ou sobrescrever um arquivo remoto", "Crear o sobrescribir un archivo remoto"),
    ("Upload local files/folders", "Enviar arquivos/pastas locais", "Subir archivos/carpetas locales"),
    ("Download remote files/folders", "Baixar arquivos/pastas remotos", "Descargar archivos/carpetas remotos"),
    ("Transfer queue status", "Status da fila de transferências", "Estado de la cola de transferencias"),
    ("Cancel a queued or running transfer", "Cancelar uma transferência na fila ou em andamento", "Cancelar una transferencia en cola o en curso"),
    ("List a directory on this computer", "Listar um diretório neste computador", "Listar un directorio en este equipo"),
    // ---------------------------------------------------------------- settings: general
    ("Language and how the file panes behave.", "Idioma e comportamento dos painéis de arquivos.", "Idioma y comportamiento de los paneles de archivos."),
    ("Language", "Idioma", "Idioma"),
    ("Automatic (system)", "Automático (sistema)", "Automático (sistema)"),
    ("Automatic follows the system language (currently {lang}).", "Automático segue o idioma do sistema (atualmente {lang}).", "Automático sigue el idioma del sistema (ahora {lang})."),
    ("File panes", "Painéis de arquivos", "Paneles de archivos"),
    ("Show hidden files", "Mostrar arquivos ocultos", "Mostrar archivos ocultos"),
    ("Show dotfiles (e.g. .env, .htaccess) when a pane opens. Toggle per pane with the eye button.", "Mostrar arquivos que começam com ponto (ex.: .env, .htaccess) ao abrir um painel. Alterne em cada painel com o botão de olho.", "Mostrar archivos que empiezan por punto (p. ej. .env, .htaccess) al abrir un panel. Cámbialo en cada panel con el botón del ojo."),
    ("Double-clicking a file", "Duplo clique em um arquivo", "Doble clic en un archivo"),
    ("Transfer it to the other side, or open local files with their default app.", "Transfere para o outro lado ou abre arquivos locais no app padrão.", "Lo transfiere al otro lado o abre los archivos locales con su app predeterminada."),
    ("Transfer", "Transferir", "Transferir"),
    ("Confirm before deleting", "Confirmar antes de excluir", "Confirmar antes de eliminar"),
    ("Ask before deleting local or remote files and folders.", "Perguntar antes de excluir arquivos e pastas locais ou remotos.", "Preguntar antes de eliminar archivos y carpetas locales o remotos."),
    ("Open the transfer panel", "Abrir o painel de transferências", "Abrir el panel de transferencias"),
    ("Expand the transfer panel when a transfer starts.", "Expandir o painel de transferências quando uma transferência começar.", "Expandir el panel de transferencias cuando empieza una transferencia."),
    ("Data", "Dados", "Datos"),
    ("Settings folder", "Pasta de configurações", "Carpeta de configuración"),
    // ---------------------------------------------------------------- settings: connection
    ("Timeouts, retries and protocol options. Changes apply to new connections.", "Tempos limite, novas tentativas e opções de protocolo. As mudanças valem para novas conexões.", "Tiempos de espera, reintentos y opciones de protocolo. Los cambios se aplican a nuevas conexiones."),
    ("Timeout", "Tempo limite", "Tiempo de espera"),
    ("Give up connecting or logging in after this long.", "Desistir de conectar ou fazer login após esse tempo.", "Dejar de intentar conectar o iniciar sesión pasado este tiempo."),
    ("Maximum number of retries", "Número máximo de tentativas", "Número máximo de reintentos"),
    ("Extra attempts when a connection or login fails.", "Tentativas extras quando uma conexão ou login falha.", "Intentos adicionales cuando falla una conexión o un inicio de sesión."),
    ("Delay between failed attempts", "Intervalo entre tentativas", "Espera entre intentos fallidos"),
    ("Wait before retrying a failed connection, login or transfer.", "Esperar antes de repetir uma conexão, login ou transferência que falhou.", "Esperar antes de reintentar una conexión, inicio de sesión o transferencia fallida."),
    ("Reconnect automatically", "Reconectar automaticamente", "Reconectar automáticamente"),
    ("When the server drops the connection, reconnect and retry the operation once.", "Quando o servidor derrubar a conexão, reconectar e repetir a operação uma vez.", "Si el servidor corta la conexión, reconectar y reintentar la operación una vez."),
    ("Keep-alive interval", "Intervalo de keep-alive", "Intervalo de keep-alive"),
    ("Send a keep-alive when the connection is idle, so routers and servers don't drop it. 0 turns it off.", "Enviar um keep-alive quando a conexão estiver ociosa, para que roteadores e servidores não a derrubem. 0 desativa.", "Enviar un keep-alive cuando la conexión está inactiva, para que routers y servidores no la corten. 0 lo desactiva."),
    ("Transfer mode", "Modo de transferência", "Modo de transferencia"),
    ("Passive works behind most firewalls. Active needs the server to reach this computer.", "O passivo funciona atrás da maioria dos firewalls. O ativo exige que o servidor alcance este computador.", "El pasivo funciona detrás de la mayoría de los firewalls. El activo requiere que el servidor llegue a este equipo."),
    ("Passive", "Passivo", "Pasivo"),
    ("Extended passive", "Passivo estendido", "Pasivo extendido"),
    ("Active", "Ativo", "Activo"),
    ("Fix passive-mode addresses", "Corrigir endereços do modo passivo", "Corregir direcciones del modo pasivo"),
    ("Use the server's address when it replies to PASV with a private IP (common behind NAT).", "Usar o endereço do servidor quando ele responder ao PASV com um IP privado (comum atrás de NAT).", "Usar la dirección del servidor cuando responde a PASV con una IP privada (habitual detrás de NAT)."),
    // ---------------------------------------------------------------- settings: transfers
    ("Overwrite", "Sobrescrever", "Sobrescribir"),
    ("Always replace the existing file.", "Sempre substituir o arquivo existente.", "Reemplazar siempre el archivo existente."),
    ("Overwrite if newer", "Sobrescrever se for mais novo", "Sobrescribir si es más reciente"),
    ("Replace only when the source file is newer.", "Substituir só quando o arquivo de origem for mais novo.", "Reemplazar solo si el archivo de origen es más reciente."),
    ("Overwrite if different", "Sobrescrever se for diferente", "Sobrescribir si es diferente"),
    ("Replace when size or date differ (like a sync).", "Substituir quando o tamanho ou a data forem diferentes (como uma sincronização).", "Reemplazar si el tamaño o la fecha difieren (como una sincronización)."),
    ("Keep both: the new copy becomes \"name (1).ext\".", "Manter os dois: a nova cópia vira \"nome (1).ext\".", "Conservar ambos: la nueva copia se llama \"nombre (1).ext\"."),
    ("Skip", "Ignorar", "Omitir"),
    ("Leave the existing file and skip this one.", "Manter o arquivo existente e ignorar este.", "Conservar el archivo existente y omitir este."),
    ("Queue concurrency, speed limits and what happens when a file already exists.", "Transferências simultâneas, limites de velocidade e o que fazer quando um arquivo já existe.", "Transferencias simultáneas, límites de velocidad y qué hacer cuando un archivo ya existe."),
    ("Queue", "Fila", "Cola"),
    ("Maximum simultaneous transfers", "Máximo de transferências simultâneas", "Máximo de transferencias simultáneas"),
    ("How many files are transferred at the same time. Applies immediately.", "Quantos arquivos são transferidos ao mesmo tempo. Vale imediatamente.", "Cuántos archivos se transfieren a la vez. Se aplica de inmediato."),
    ("Retries for failed transfers", "Tentativas para transferências com falha", "Reintentos de transferencias fallidas"),
    ("Retry a file this many times (after the delay set in Connection) before marking it failed.", "Tentar um arquivo este número de vezes (após o intervalo definido em Conexão) antes de marcá-lo como falho.", "Reintentar un archivo este número de veces (tras la espera definida en Conexión) antes de marcarlo como fallido."),
    ("Preserve timestamps", "Preservar datas de modificação", "Conservar fechas de modificación"),
    ("Give the copy the same modification time as the original (needed for the compare badges and \"overwrite if newer\").", "Dar à cópia a mesma data de modificação do original (necessário para os selos de comparação e \"sobrescrever se for mais novo\").", "Dar a la copia la misma fecha de modificación que el original (necesario para las etiquetas de comparación y \"sobrescribir si es más reciente\")."),
    ("Speed limits", "Limites de velocidade", "Límites de velocidad"),
    ("Download limit", "Limite de download", "Límite de descarga"),
    ("Shared by all downloads. Applies immediately, also to running transfers.", "Compartilhado por todos os downloads. Vale imediatamente, inclusive para transferências em andamento.", "Compartido por todas las descargas. Se aplica de inmediato, también a las transferencias en curso."),
    ("Upload limit", "Limite de envio", "Límite de subida"),
    ("Shared by all uploads.", "Compartilhado por todos os envios.", "Compartido por todas las subidas."),
    ("When the file already exists", "Quando o arquivo já existe", "Cuando el archivo ya existe"),
    ("Unlimited", "Ilimitado", "Ilimitado"),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashSet};

    fn placeholders(s: &str) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        let mut rest = s;
        while let Some(i) = rest.find('{') {
            rest = &rest[i + 1..];
            if let Some(j) = rest.find('}') {
                out.insert(rest[..j].to_string());
                rest = &rest[j + 1..];
            }
        }
        out
    }

    #[test]
    fn keys_unique_and_translated() {
        let mut seen = HashSet::new();
        for (en, pt, es) in STRINGS {
            assert!(seen.insert(*en), "duplicate key: {en:?}");
            assert!(!pt.is_empty() && !es.is_empty(), "empty translation for {en:?}");
            assert_eq!(placeholders(en), placeholders(pt), "pt-BR placeholders differ for {en:?}");
            assert_eq!(placeholders(en), placeholders(es), "es placeholders differ for {en:?}");
        }
    }

    /// Unescape the contents of a Rust string literal (only the escapes the UI uses).
    fn unescape(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(o) => out.push(o),
                    None => {}
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Every `tr("…")` / `trf("…")` literal in the UI sources must have a translation.
    #[test]
    fn every_ui_string_is_translated() {
        let sources = [
            include_str!("ui/assets.rs"),
            include_str!("ui/components.rs"),
            include_str!("ui/dialogs.rs"),
            include_str!("ui/edit.rs"),
            include_str!("ui/input.rs"),
            include_str!("ui/mod.rs"),
            include_str!("ui/panes.rs"),
            include_str!("ui/rail.rs"),
            include_str!("ui/settings.rs"),
            include_str!("ui/settings_pages.rs"),
            include_str!("ui/shortcuts.rs"),
            include_str!("ui/shortcuts_page.rs"),
            include_str!("ui/theme.rs"),
            include_str!("ui/titlebar.rs"),
            include_str!("ui/updates.rs"),
            include_str!("ui/workspace.rs"),
        ];
        let mut missing = vec![];
        let mut found = 0;
        for src in sources {
            for (i, _) in src.match_indices("tr") {
                let before = src[..i].chars().next_back();
                if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let rest = &src[i + 2..];
                let rest = rest.strip_prefix('f').unwrap_or(rest);
                // `tr(` may be followed by a line break before the literal
                let Some(lit) = rest.strip_prefix('(').map(str::trim_start).and_then(|r| r.strip_prefix('"')) else { continue };
                let mut end = None;
                let mut escaped = false;
                for (j, c) in lit.char_indices() {
                    match c {
                        _ if escaped => escaped = false,
                        '\\' => escaped = true,
                        '"' => {
                            end = Some(j);
                            break;
                        }
                        _ => {}
                    }
                }
                let key = unescape(&lit[..end.expect("unterminated literal")]);
                found += 1;
                if !INDEX.contains_key(key.as_str()) {
                    missing.push(key);
                }
            }
        }
        assert!(found > 100, "scanner found only {found} strings");
        assert!(missing.is_empty(), "untranslated UI strings: {missing:#?}");
    }

    #[test]
    fn mcp_tool_descriptions_are_translated() {
        for (name, desc) in crate::mcp::TOOLS {
            assert!(INDEX.contains_key(desc), "no translation for the description of {name}: {desc:?}");
        }
    }

    #[test]
    fn lookup_and_fallback() {
        assert_eq!(tr_in(Lang::En, "Refresh"), "Refresh");
        assert_eq!(tr_in(Lang::PtBr, "Refresh"), "Atualizar");
        assert_eq!(tr_in(Lang::Es, "Refresh"), "Actualizar");
        assert_eq!(tr_in(Lang::PtBr, "not a key"), "not a key");
        assert_eq!(fill(tr_in(Lang::PtBr, "{n} selected"), &[("n", "3")]), "3 selecionados");
        assert_eq!(fill("{a} and {b}", &[("a", "x"), ("b", "y")]), "x and y");
    }

    #[test]
    fn settings_resolve() {
        assert_eq!(resolve("en"), Lang::En);
        assert_eq!(resolve("pt-BR"), Lang::PtBr);
        assert_eq!(resolve("es"), Lang::Es);
        assert_eq!(resolve("klingon"), Lang::En);
        assert_eq!(from_locale(Some("pt-BR")), Lang::PtBr);
        assert_eq!(from_locale(Some("es-MX")), Lang::Es);
        assert_eq!(from_locale(Some("de-DE")), Lang::En);
        assert_eq!(from_locale(None), Lang::En);
        for l in Lang::ALL {
            assert_eq!(resolve(l.code()), l);
            assert_eq!(Lang::from_u8(l.as_u8()), l);
        }
    }
}
