#!/usr/bin/env node
'use strict'

const { readFile, writeFile } = require('node:fs/promises')

const FORMATS = 'doc, docx, odt, pdf, ppt, pptx, rtf, epub, xlsx, ods, odp, csv'

const OCR_BACKENDS = 'tesseract, auto, none'

const HELP = `anydoc: convert documents to GitHub-Flavored Markdown

Usage:
  anydoc <file> [options]
  anydoc - [options] < file

Converts one document per invocation and writes the Markdown to stdout.
Pass - as the input to read the document from stdin. Never prompts; all
diagnostics go to stderr.

Options:
  -o, --output <path>    Write the Markdown to <path> instead of stdout
  -f, --format <format>  Name the input format instead of detecting it:
                         ${FORMATS}
                         (extension aliases like xls, docm, ppsx resolve
                         to these)
  --ocr <backend>        OCR fallback for scanned/image-only PDFs:
                         ${OCR_BACKENDS}
                         (default: none)
  -h, --help             Print this help and exit
  -V, --version          Print the version and exit

The format is detected from the file content; the file extension is the
fallback for signature-less formats (CSV). stdin has no extension, so CSV
input from stdin needs --format csv.

When --ocr is set, pages that pdf-inspector flags as having no text layer
are rendered to images and run through the specified OCR backend.
'auto' picks the first available engine. Text-based PDFs are never sent
through OCR, so the fast path stays fast.

Exit codes:
  0  success
  1  the document could not be read or converted
  2  usage error: unknown option, missing input, or invalid --format

Examples:
  anydoc report.docx
  anydoc slides.pptx -o slides.md
  anydoc - --format csv < data.csv
  anydoc scanned.pdf --ocr tesseract
  curl -s https://example.com/paper.pdf | anydoc - --ocr auto
`

const USAGE_ERROR = 2
const CONVERSION_ERROR = 1

function fail(code, message) {
  process.stderr.write(`anydoc: ${message}\n`)
  process.exit(code)
}

function parseArgs(argv) {
  const args = { input: null, output: null, format: null, ocr: null }
  let positionalOnly = false
  for (let i = 0; i < argv.length; i++) {
    let arg = argv[i]
    if (positionalOnly || arg === '-' || !arg.startsWith('-')) {
      if (args.input !== null) {
        fail(USAGE_ERROR, `one document per invocation: unexpected second input '${arg}'`)
      }
      args.input = arg
      continue
    }
    if (arg === '--') {
      positionalOnly = true
      continue
    }
    let inline = null
    const eq = arg.indexOf('=')
    if (arg.startsWith('--') && eq !== -1) {
      inline = arg.slice(eq + 1)
      arg = arg.slice(0, eq)
    }
    const value = () => {
      if (inline !== null) return inline
      if (i + 1 >= argv.length) fail(USAGE_ERROR, `${arg} requires a value`)
      return argv[++i]
    }
    switch (arg) {
      case '-h':
      case '--help':
        process.stdout.write(HELP)
        process.exit(0)
        break
      case '-V':
      case '--version':
        process.stdout.write(`${require('./package.json').version}\n`)
        process.exit(0)
        break
      case '-o':
      case '--output':
        args.output = value()
        break
      case '-f':
      case '--format':
        args.format = value()
        break
      case '--ocr':
        args.ocr = value()
        break
      default:
        fail(USAGE_ERROR, `unknown option '${arg}' (see anydoc --help)`)
    }
  }
  return args
}

async function readStdin() {
  if (process.stdin.isTTY) {
    fail(USAGE_ERROR, 'stdin is a terminal; pipe or redirect a document into anydoc -')
  }
  const chunks = []
  for await (const chunk of process.stdin) {
    chunks.push(chunk)
  }
  return Buffer.concat(chunks)
}

async function main() {
  const args = parseArgs(process.argv.slice(2))
  if (args.input === null) {
    fail(USAGE_ERROR, 'missing input: pass a document path, or - for stdin (see anydoc --help)')
  }

  // Loaded after argument handling so --help and --version work even where
  // no native binding is available.
  const { formatFromExtension, toMarkdown, toMarkdownBytes, toMarkdownWithOcr } = require('./index.js')

  let format
  if (args.format !== null) {
    format = formatFromExtension(args.format)
    if (format === null) {
      fail(USAGE_ERROR, `invalid format '${args.format}'; expected one of: ${FORMATS}`)
    }
  }

  // Validate --ocr value
  const validOcr = ['tesseract', 'auto', 'none', null]
  if (!validOcr.includes(args.ocr)) {
    fail(USAGE_ERROR, `invalid --ocr '${args.ocr}'; expected one of: ${OCR_BACKENDS}`)
  }

  // Read input bytes
  let bytes
  if (args.input === '-') {
    bytes = await readStdin()
  } else {
    bytes = await readFile(args.input)
  }

  let markdown
  try {
    if (args.ocr && args.ocr !== 'none') {
      // OCR fallback path: the native module exposes toMarkdownWithOcr
      // which takes (bytes, format, ocrBackend) and handles the rest.
      if (typeof toMarkdownWithOcr !== 'function') {
        fail(USAGE_ERROR,
          `--ocr is not supported by this build of anydoc. ` +
          `Rebuild with: cargo build --features ocr-tesseract`)
      }
      markdown = await toMarkdownWithOcr(bytes, format, args.ocr)
    } else if (args.input !== '-' && format === undefined) {
      markdown = await toMarkdown(args.input)
    } else {
      markdown = await toMarkdownBytes(bytes, format)
    }
  } catch (error) {
    fail(CONVERSION_ERROR, error.message)
  }

  if (args.output !== null) {
    try {
      await writeFile(args.output, markdown)
    } catch (error) {
      fail(CONVERSION_ERROR, error.message)
    }
  } else {
    // Downstream closing the pipe early (e.g. `anydoc big.xlsx | head`) is
    // not a conversion failure.
    process.stdout.on('error', (error) => {
      process.exit(error.code === 'EPIPE' ? 0 : CONVERSION_ERROR)
    })
    process.stdout.write(markdown)
  }
}

main()
