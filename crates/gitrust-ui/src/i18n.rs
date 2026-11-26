use crate::theme::FileStatus;

/// Language used by the application shell and diagnostics.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Language {
    /// English (United States), also used as the fallback.
    #[default]
    English,
    /// Brazilian Portuguese.
    Portuguese,
}

impl Language {
    /// Resolves a supported locale; unknown identifiers fall back to English.
    #[must_use]
    pub fn from_locale(locale: &str) -> Self {
        if locale.eq_ignore_ascii_case("pt-BR") {
            Self::Portuguese
        } else {
            Self::English
        }
    }

    pub(crate) const fn text(self, key: Text) -> &'static str {
        let (english, portuguese) = key.strings();
        self.localized(english, portuguese)
    }

    pub(crate) const fn local(
        self,
        english: &'static str,
        portuguese: &'static str,
    ) -> &'static str {
        self.localized(english, Some(portuguese))
    }

    const fn localized(
        self,
        english: &'static str,
        portuguese: Option<&'static str>,
    ) -> &'static str {
        match (self, portuguese) {
            (Self::Portuguese, Some(translated)) => translated,
            _ => english,
        }
    }
}

macro_rules! catalog {
    ($($name:ident => $en:literal, $pt:literal;)*) => {
        #[derive(Clone, Copy, Debug)]
        pub(crate) enum Text { $($name,)* }
        impl Text {
            const fn strings(self) -> (&'static str, Option<&'static str>) {
                match self { $(Self::$name => ($en, Some($pt)),)* }
            }
        }
    };
}

catalog! {
    File => "File", "Arquivo";
    Edit => "Edit", "Editar";
    View => "View", "Exibir";
    Repository => "Repository", "Repositório";
    Branch => "Branch", "Branch";
    Help => "Help", "Ajuda";
    Settings => "Settings", "Configurações";
    About => "About gitRUST", "Sobre o gitRUST";
    Workspace => "Workspace", "Área de trabalho";
    NoRepositoryShort => "None open", "Nenhum aberto";
    Unavailable => "Unavailable", "Indisponível";
    ChangedFiles => "changed files", "arquivos alterados";
    OneChangedFile => "changed file", "arquivo alterado";
    CommitCount => "commits", "commits";
    OneCommit => "commit", "commit";
    DraftOnly => "Local draft. Commit unavailable.", "Rascunho local. Commit indisponível.";
    OptionalDescription => "Description (optional)", "Descrição (opcional)";
    NoRepository => "No repository open", "Nenhum repositório aberto";
    Intro => "Git integration is coming in a future stage. You can set the theme, language and interface scale in Settings.", "A integração com Git chegará em uma etapa futura. Você pode ajustar tema, idioma e escala da interface em Configurações.";
    NoBranch => "No branch", "Sem branch";
    Changes => "Changes", "Alterações";
    History => "History", "Histórico";
    Remote => "Remote", "Remoto";
    Fetch => "Fetch", "Buscar (fetch)";
    Pull => "Pull", "Baixar (pull)";
    Push => "Push", "Enviar (push)";
    NoGit => "Git integration unavailable", "Integração com Git indisponível";
    Demo => "Demonstration", "Demonstração";
    DemoRepo => "Demo repository", "Repositório de demonstração";
    DemoHint => "Sample data. Git actions are unavailable.", "Dados de exemplo. Operações Git indisponíveis.";
    Gallery => "Component gallery", "Galeria de componentes";
    Find => "Filter list", "Filtrar lista";
    Filters => "Clear filter", "Limpar filtro";
    NoMatches => "No matching entries", "Nenhum resultado";
    NoChanges => "No local changes", "Nenhuma alteração local";
    NoHistory => "No commits to show", "Nenhum commit para exibir";
    PickFile => "Select a file to review its diff.", "Selecione um arquivo para revisar as diferenças.";
    PickCommit => "Select a commit to see its details.", "Selecione um commit para ver os detalhes.";
    Summary => "Commit summary", "Resumo do commit";
    Description => "Description", "Descrição";
    Compose => "Commit message", "Mensagem de commit";
    Commit => "Commit", "Criar commit";
    Unified => "Unified", "Unificado";
    Split => "Split", "Lado a lado";
    DiffViewport => "Diff scroll area", "Área de rolagem do diff";
    OldLine => "Before, line", "Antes, linha";
    NewLine => "After, line", "Depois, linha";
    Diff => "Diff", "Diferenças";
    Old => "Before", "Antes";
    New => "After", "Depois";
    Binary => "Binary file. Text diff unavailable.", "Arquivo binário. Diff de texto indisponível.";
    EmptyDiff => "This file has no text changes.", "Este arquivo não tem alterações de texto.";
    Copy => "Copy", "Copiar";
    CopyFullDiff => "Copy entire diff", "Copiar diff completo";
    Copied => "Diff copied", "Diff copiado";
    Theme => "Theme", "Tema";
    System => "System", "Sistema";
    Light => "Light", "Claro";
    Dark => "Dark", "Escuro";
    Language => "Language", "Idioma";
    Scale => "Interface scale", "Escala da interface";
    Close => "Close", "Fechar";
    Cancel => "Cancel", "Cancelar";
    Confirm => "Confirm", "Confirmar";
    Runtime => "Runtime", "Ambiente de execução";
    Telemetry => "Telemetry off", "Telemetria desativada";
    Added => "Added", "Adicionado";
    Modified => "Modified", "Modificado";
    Deleted => "Deleted", "Excluído";
    Renamed => "Renamed", "Renomeado";
    Untracked => "Untracked", "Não rastreado";
    Conflicted => "Conflicted", "Em conflito";
    Staged => "Staged", "Preparado";
    Unstaged => "Unstaged", "Fora do stage";
    Ignored => "Ignored", "Ignorado";
    Author => "Author", "Autor";
    Date => "Date", "Data";
    Revision => "Revision", "Revisão";
    Typography => "Typography", "Tipografia";
    Background => "Background", "Fundo";
    Surface => "Surface", "Superfície";
    Elevated => "Elevated surface", "Superfície elevada";
    Selected => "Selected surface", "Superfície selecionada";
    Border => "Border", "Borda";
    Divider => "Divider", "Divisor";
    PrimaryForeground => "Primary text", "Texto principal";
    SecondaryForeground => "Secondary text", "Texto secundário";
    DisabledForeground => "Disabled text", "Texto desabilitado";
    Accent => "Accent", "Destaque";
    OnAccent => "Text on accent", "Texto sobre destaque";
    Focus => "Focus", "Foco";
    Hover => "Hover", "Sob o ponteiro";
    Active => "Pressed", "Pressionado";
    Selection => "Text selection", "Seleção de texto";
    Success => "Success", "Sucesso";
    Warning => "Warning", "Aviso";
    ErrorTone => "Error", "Erro";
    Information => "Information", "Informação";
    AddedLine => "Added line", "Linha adicionada";
    RemovedLine => "Removed line", "Linha removida";
    AddedEdit => "Added highlight", "Realce de adição";
    RemovedEdit => "Removed highlight", "Realce de remoção";
    Colors => "Color tokens", "Tokens de cor";
    Controls => "Controls", "Controles";
    States => "UI states", "Estados da interface";
    Thousand => "1,000", "1.000";
    TenThousand => "10,000", "10.000";
    StressDiff => "100,000 lines", "100.000 linhas";
    Lists => "Large lists", "Listas extensas";
    Loading => "Loading", "Carregando";
    Working => "Operation in progress", "Operação em andamento";
    Error => "Unable to load this preview", "Não foi possível carregar esta prévia";
    Conflict => "This file has a conflict", "Este arquivo tem um conflito";
    ConflictDetails => "Conflict resolution will be available with Git integration.", "A resolução de conflitos estará disponível com a integração Git.";
    Destructive => "Discard draft?", "Descartar rascunho?";
    Discard => "Discard", "Descartar";
    DiscardDraft => "Discard draft", "Descartar rascunho";
    DiscardDetails => "This clears the sample commit message in this window.", "Isso apaga a mensagem de commit de exemplo nesta janela.";
    Retry => "Reset preview", "Redefinir prévia";
    Example => "Example", "Exemplo";
    Toggle => "Toggle", "Alternância";
    Checkbox => "Checkbox", "Caixa de seleção";
    Radio => "Radio button", "Botão de opção";
    Menu => "Menu", "Menu";
    ContextHelp => "Right-click for options.", "Clique com o botão direito para ver opções.";
    Count => "Entries", "Itens";
    ShowList => "Show list", "Mostrar lista";
    ShowDetail => "Show details", "Mostrar detalhes";
}

impl FileStatus {
    pub(crate) const fn label(self) -> Text {
        match self {
            Self::Added => Text::Added,
            Self::Modified => Text::Modified,
            Self::Deleted => Text::Deleted,
            Self::Renamed => Text::Renamed,
            Self::Untracked => Text::Untracked,
            Self::Conflicted => Text::Conflicted,
            Self::Staged => Text::Staged,
            Self::Unstaged => Text::Unstaged,
            Self::Ignored => Text::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_fallback_and_accented_text() {
        assert_eq!(
            Language::from_locale("pt-br").text(Text::Settings),
            "Configurações"
        );
        assert_eq!(
            Language::from_locale("es-ES").text(Text::Settings),
            "Settings"
        );
        assert_eq!(Language::Portuguese.localized("Settings", None), "Settings");
    }
}
