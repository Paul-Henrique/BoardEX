# Amostras sintéticas

Estes arquivos são **fictícios**, criados apenas para desenvolvimento e testes de regressão dos parsers. Não representam nenhuma placa real.

| Arquivo | Formato exercitado |
|---|---|
| `demo.brd2` | BRD2 completo, com componentes top/bottom, pinos e nails |
| `demo.brd` | BRD com cabeçalho comprimido/ofuscado |
| `demo.bvr` | BVR `BVRAW_FORMAT_1` com layout separado por vírgulas |
| `demo.bvr3` | BVR3 com contorno e pinos por origem relativa |
| `demo.bvr2` | BVR2 com layout, pinos, valores e nails |
| `demo.fbv` | Corpo BVR2 cifrado em FBV (XOR com chave não nula) |
| `demo.bdv` | BDV com texto ofuscado |
| `demo.fz` | FZ sem cifra, com REFDES, NET_NAME, TESTVIA, unidades em mil e descrições |

`demo.fz` é um contêiner FZ sintético **sem cifra** (zlib). Ele não usa RC6 e, portanto, abre sem chave.

Para testar FZ cifrado é necessário um arquivo real acompanhado da chave de 44 palavras. Use **Chave FZ…** na interface, ou:

```bash
cargo run -- --fz-key-file obv.conf caminho/placa.fz
```

O BoardEX também aceita a pasta do pacote em vez do arquivo `.fz`; ele procura recursivamente até 4 níveis e usa o arquivo quando há exatamente um `.fz`.

Para validar a leitura sem abrir a janela:

```bash
cargo run -- --inspect samples/demo.bvr3
cargo run -- --inspect samples/demo.fz
```

Para abrir uma amostra na interface:

```bash
cargo run -- samples/demo.brd2
```
