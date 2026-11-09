# Vapora Launcher 0.3.0

Launcher desktop local para Windows e Linux, com catálogo visual, biblioteca, repositórios JSON, downloads HTTPS e motor BitTorrent integrado (aria2).

## O que esta versão faz

- Catálogo e biblioteca locais em SQLite.
- Repositórios HTTPS com sincronização de catálogo JSON.
- Fontes por jogo: `direct` (HTTPS), `magnet` (magnet URI) e `torrent` (arquivo .torrent em HTTPS).
- Downloads diretos com progresso e verificação SHA-256 opcional.
- Downloads BitTorrent com progresso consultado pelo RPC local protegido por token aleatório.
- Extração segura de arquivos ZIP (rejeita caminhos de saída fora da pasta escolhida).
- Cadastro e inicialização manual de jogos já instalados.
- Sem conta, backend remoto ou telemetria.

## Limites explícitos da primeira versão

- Não instala silenciosamente executáveis de terceiros. Instaladores `.exe` devem ser iniciados pelo usuário e sob sua responsabilidade.
- A extração automática nesta versão é para ZIP; instaladores próprios, ISO, 7z, RAR e pacotes multi-volume exigem módulos específicos futuros.
- Não há pausa/retomada pela interface, fila paralela configurável, detecção automática de jogos, atualização de jogos, cloud saves ou integração com Steam.
- O catálogo não inclui fontes prontas. O usuário configura seus próprios repositórios e deve usar somente arquivos que possui ou tem autorização para baixar/distribuir.

## Formato de catálogo

Veja `examples/catalog.example.json`. O arquivo remoto deve ser JSON com uma propriedade `games`. Cada jogo aceita `title`, `developer`, `description`, `cover` e `sources`. Cada fonte aceita `name`, `url`, `kind`, `size` e `sha256`.

Tipos de fonte:

- `direct`: URL HTTPS de um arquivo.
- `magnet`: URI `magnet:`.
- `torrent`: URL HTTPS de um arquivo `.torrent`.

Hospede o JSON em uma URL HTTPS, por exemplo GitHub Raw, e adicione essa URL na tela Repositórios. Não use conteúdo de terceiros sem autorização.

## Como gerar os aplicativos sem instalar ferramentas no seu PC

1. Envie este projeto para um repositório GitHub seu.
2. Abra a aba **Actions** e habilite workflows se o GitHub solicitar.
3. Execute **Build Vapora** em **Run workflow**.
4. Aguarde os jobs Windows e Linux terminarem.
5. Baixe os artefatos `Vapora-Windows-x64` ou `Vapora-Linux-x64`.

O workflow baixa aria2 1.37.0 para Windows e verifica o SHA-256 antes de empacotar. No Linux, usa um build estático x86_64 de aria2 1.37.0 do projeto dmesg00/aria2-static-builds, para não depender de aria2 instalado no computador do usuário. O workflow também inclui o pacote de certificados CA usado pelo aria2 em conexões HTTPS. O binário aria2 é distribuído sem modificação; consulte `THIRD_PARTY_NOTICES.md`.

## Desenvolvimento local

Requer Node.js 22+, Rust estável e dependências do Tauri 2. O usuário final não precisa instalar essas ferramentas: elas são necessárias apenas para compilar o código-fonte.

```bash
npm install
npm run tauri dev
npm run tauri build
```

## Privacidade e segurança

O banco de dados fica no diretório de dados do aplicativo. URLs de repositório e download precisam usar HTTPS, exceto magnet URI. O motor BitTorrent só abre RPC em `127.0.0.1` e usa token aleatório por processo. Não adicione repositórios desconhecidos. SHA-256 valida integridade em downloads diretos quando o catálogo fornece um hash confiável; em torrents, o protocolo verifica as peças.

## Licenças

O código do Vapora está sob MIT. Componentes de terceiros são identificados em `THIRD_PARTY_NOTICES.md`.

## Fluxo de instalação após o download

1. Escolha a fonte do jogo e baixe para uma pasta de sua preferência.
2. Se o arquivo for ZIP, use **Extrair ZIP** na página Downloads e selecione a pasta de instalação.
3. Para instaladores `.exe`, abra a pasta do arquivo e execute o instalador manualmente, conferindo editor e origem.
4. Depois, em **Biblioteca > Adicionar jogo**, selecione a pasta e o executável principal para iniciar o jogo pelo Vapora.
