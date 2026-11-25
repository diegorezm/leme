# Auditoria Técnica e Roadmap — Leme

Registro da auditoria do código-fonte (workspace `domain`, `web`, `pdf`, `app` e interfaces `.slint`) e plano de correções, otimizações e evoluções alinhadas à filosofia do projeto (`AGENTS.md`).

---

## 1. Bugs e potenciais causadores de falhas

### Prioridade alta

- [x] **Race condition de geração entre documentos e `PageTint`**
  - **Onde:** `crates/pdf/src/lib.rs`, `crates/pdf/src/worker.rs`, `crates/app/src/viewer/documents.rs`, `crates/app/src/viewer/rendering.rs`, `crates/app/src/viewer/side_panel.rs`, `crates/app/src/app/events.rs`
  - **Causa:** `Event::Rendered`, `Event::Thumbnail`, `Event::Labels`, `Event::Selection`, `Event::Link` e `Event::Article` não carregam identificador de geração de documento/tint. Após `reset_document_state` ou `set_page_tint`, um evento já enfileirado em `invoke_from_event_loop` pode entrar no cache do novo documento/tint e permanecer visível como se fosse da renderização atual. Além disso, `Event::Error` de falha de renderização de página aciona `open_failed`, podendo fechar outra aba caso exista uma abertura pendente.
  - **Correção:** propagar um `generation: u64` (e separar `OpenFailed` de `Error` genérico).

- [x] **`Request::Open` não limpa `select_line` no worker**
  - **Onde:** `crates/pdf/src/worker.rs` (`run`)
  - **Causa:** ao abrir um novo documento, `renders`, `thumbs`, `search`, `last_search`, `select`, `link`, `article`, `char_cache` e `labels_due` são limpos, mas `select_line` fica pendente. Um duplo-clique seguido imediatamente de troca de aba executa `SelectLine` no documento errado.

- [x] **Possível panic/OOM em `typeset::merge` quando `space(p.face) == 0.0`**
  - **Onde:** `crates/pdf/src/typeset.rs` (`merge`)
  - **Causa:** `gap / space(p.face)` resulta em `f32::INFINITY` se o avanço do espaço for `0.0`; a conversão `as usize` satura em `usize::MAX` e `" ".repeat(spaces)` causa panic/OOM.
  - **Correção:** validar `sp > 0.0` e limitar a quantidade máxima de espaços inseridos.

- [x] **Blocos `<pre>` (`Kind::Code`) perdem quebras de linha e indentação no typesetter**
  - **Onde:** `crates/pdf/src/typeset.rs` (`break_lines` / `write`)
  - **Causa:** `html::extract` preserva espaços e quebras de linha em `<pre>`, mas `typeset::break_lines` sempre processa `text.split_whitespace()`, colapsando toda a formatação de código.

- [x] **Links externos e "Abrir pasta/config" falham no Linux**
  - **Onde:** `crates/app/src/app/mod.rs` (`reveal`), `crates/app/src/app/events.rs` (`Event::Link`)
  - **Causa:** ambos invocam `Command::new("explorer.exe")` sem fallback `xdg-open` fora do Windows (diferente de `crates/web/src/update.rs::open_page`).

### Prioridade média

- [x] **Deslocamento ou perda da aba ativa em `load_session` quando um arquivo sumiu do disco**
  - **Onde:** `crates/app/src/app/sessions.rs` (`load_session`)
  - **Causa:** `session.tabs` é filtrado com `.exists()` antes de resolver `session.active`, deslocando o índice ou caindo na tela inicial se a aba ativa era a última.

- [x] **Caminhos relativos via CLI geram chaves incompatíveis de sessão/anotações**
  - **Onde:** `crates/app/src/main.rs`
  - **Causa:** `instance::encode` converte argumentos para caminhos absolutos, mas `main()` passa os caminhos crus para `app.open_path`.

- [x] **`page_for_library` vaza quando `Event::Typeset` retorna `Err`**
  - **Onde:** `crates/app/src/app/events.rs` (`Event::Typeset`)
  - **Causa:** `self.page_for_library` só é consumido no ramo `Ok(())`; em caso de erro, a próxima página salva/atualizada pode ser adicionada à biblioteca pendente antiga.

- [x] **Deletar biblioteca ativa ou criar biblioteca inválida corrompe índice selecionado**
  - **Onde:** `crates/app/src/app/libraries.rs` (`library_action`, `library_setting`), `crates/app/ui/settings.slint`
  - **Causa:** remover uma biblioteca não ajusta nem fecha `self.library_view`, fazendo a tela pular silenciosamente para outra biblioteca; em Settings, `library-new` seta `library-selected = library-rows.length` mesmo se a criação falhar ou reusar existente.

- [x] **`untr(value)` aplicado a campos de texto livre em `App::setting`**
  - **Onde:** `crates/app/src/app/settings.rs` (`setting`)
  - **Causa:** `untr` é aplicado antes do `match key`, podendo alterar nomes customizados de cor, fonte ou tema caso coincidam com alguma tradução em `leme.po`.

- [x] **Escritas não atômicas em `Libraries::save`, `web::save` e `typeset::write`**
  - **Onde:** `crates/domain/src/library.rs`, `crates/web/src/lib.rs`, `crates/pdf/src/typeset.rs`
  - **Causa:** gravam diretamente no destino em vez de escrever arquivo temporário + `rename`.

- [x] **Robustez e segurança menores**
  - `sha256` no Windows (`crates/web/src/update.rs`) chama `"certutil"` pelo `PATH` em vez de `SystemRoot\System32\certutil.exe`.
  - `yaml_escape` (`crates/domain/src/annotations.rs`) não escapa `\n`, `\r` e `\t`.
  - `local_offset` no Windows (`crates/domain/src/study.rs`) ignora segundos e não arredonda a diferença para o minuto mais próximo.
  - `same_file` (`crates/app/src/app/mod.rs`) trata caminhos como case-insensitive também no Linux.
  - `continue_reading` (`crates/app/src/app/keys.rs`) não verifica `self.library_view.is_none()`.

### Prioridade baixa / UI

- [x] **Cliques vazando para o leitor atrás dos overlays `prompt` e `update`** (`crates/app/ui/app.slint`).
- [x] **String `"Leme is up to date"` sem `tr(...)`** (`crates/app/src/app/updates.rs` + `translations/pt-BR/LC_MESSAGES/leme.po`).
- [x] **`font-family: "Cascadia Mono, Consolas"` no preview de exportação** (`crates/app/ui/app.slint`), sintaxe CSS não suportada como lista de fallback pelo Slint.
- [x] **Campo `rename` de biblioteca em Settings só salva com `Enter`**, sem salvar ao perder foco (`crates/app/ui/settings.slint`).
- [x] **Atributos HTML contendo `>` entre aspas quebram a tag em `html::tokens`** (`crates/web/src/html.rs`).

---

## 2. Otimizações

1. [x] **Outline em `O(1)` no scroll (`crates/app/src/viewer/rendering.rs` + `side_panel.rs`):** atualizar apenas a linha anterior e a nova via `set_row_data` em vez de reconstruir todo o `VecModel` do outline (até 5.000 itens) + `note_filters` a cada troca de página.
2. [x] **Índice único de fontes do sistema em `recover.rs` (`crates/pdf/src/recover.rs`):** cachear o mapa `nome_normalizado -> (PathBuf, u32)` uma vez por processo em `OnceLock`, evitando varrer e abrir todas as fontes do sistema a cada família não encontrada.
3. [x] **Menos syscalls e alocações na busca entre abas (`crates/pdf/src/find.rs`):** não chamar `modified(path)` (`stat()`) em cada página individual em `search_files_step` (checar ao abrir o arquivo) e evitar alocar `Vec<char>` + `fold` completos a cada letra digitada.
4. [x] **Parser HTML zero-copy para busca de fechamento e atributos (`crates/web/src/html.rs`):** substituir `rest.to_ascii_lowercase()` em `find_ignore_case` e `attrs.to_ascii_lowercase()` no loop de `extract` por comparação case-insensitive direta.
5. [x] **Atualização incremental de `SessionSearch`, cache de capas de biblioteca e `save_annotations` seletivo:** evitar `set_vec` completo a cada hit em `file_hit`, cachear `slint::Image` das capas de bibliotecas por `(PathBuf, mtime)`, e não salvar anotações quando `popup_action` for `"copy"`.

---

## 3. Refatorações estruturais

- [x] Helper único multiplataforma para abrir URLs/caminhos no sistema (`explorer.exe` no Windows / `xdg-open` no Linux).
- [x] `Annotations::load` retornar `(Annotations, Option<String>)` em vez de `.unwrap_or_default()` silencioso, evitando sobrescrever um TOML com erro de sintaxe manual ao adicionar uma nova nota.
- [x] Unificar `fnv1a` (hoje duplicado em `domain/src/annotations.rs` e `web/src/lib.rs`).
- [x] Em `pdf/src/text.rs::select_range`, impor limite de distância horizontal entre caracteres da mesma linha para não unir retângulos entre colunas de texto em PDFs de duas colunas.

---

## 4. Melhorias de projeto e novas features

1. **Vínculo de anotações e histórico por fingerprint do PDF** (Document `/ID` ou hash parcial rápido como fallback quando o arquivo for movido ou renomeado).
2. **Busca em fórmulas recuperadas (`recover.rs`)** integrada ao cache de texto de busca (`OpenDoc::texts`).
3. **Jumplist / histórico de navegação (`Alt+Left` / `Alt+Right`)** para voltar facilmente após clicar em links internos, sumário ou resultados de busca.
4. **Marcadores rápidos por teclado (`bookmarks`)** salvos no TOML de anotações e acessíveis via Command Palette (`Ctrl+K`).
5. **Recorte visual de margens (`crop-margins`) e rotação por documento** (não destrutivos, apenas no layout/renderização).
6. **Visualização opcional lado a lado (Two-up)** para leitura em monitores largos.
7. **Evoluções de estudo e exportação:** resumo semanal por biblioteca, exportação Anki em formato Cloze (`{{c1::...}}`) e opção de exportar apenas marcações com nota escrita; auto-reload opcional ao detectar alteração de `mtime` (útil para quem compila LaTeX/Typst).
8. **Cache em disco para capas de bibliotecas (`cache_dir`)** invalidado por `mtime`, evitando re-renderizar capas via PDFium a cada abertura.
