package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestSafeFilenameAvoidsPathsAndInvalidCharacters(t *testing.T) {
	cases := map[string]string{
		`C:\temp\evidencia?.png`:  "evidencia_.png",
		"  relatório final.pdf  ": "relatório final.pdf",
		"..":                      "anexo",
	}
	for input, want := range cases {
		if got := safeFilename(input); got != want {
			t.Fatalf("safeFilename(%q) = %q; esperado %q", input, got, want)
		}
	}
}

// Esta prova usa apenas o clone local de homologação. O caminho é recebido por
// variável de ambiente para que testes normais nunca apontem um PC real.
func TestBootstrapWithExplicitHomologationConfig(t *testing.T) {
	programData := os.Getenv("BELARC_TEST_PROGRAMDATA")
	if programData == "" {
		t.Skip("BELARC_TEST_PROGRAMDATA não definido; teste de integração isolado não executado")
	}
	oldProgramData := os.Getenv("PROGRAMDATA")
	t.Setenv("PROGRAMDATA", programData)
	t.Cleanup(func() { _ = os.Setenv("PROGRAMDATA", oldProgramData) })

	app := NewApp()
	boot := app.Bootstrap()
	if !boot.Connected {
		t.Fatalf("bootstrap local falhou: %s", boot.Message)
	}
	if boot.Hostname == "" {
		t.Fatal("bootstrap não retornou o computador identificado")
	}
	if len(boot.Departments) == 0 {
		t.Fatal("bootstrap não retornou os setores ativos")
	}
	if _, err := app.MyTickets(); err != nil {
		t.Fatalf("listagem de meus chamados falhou: %v", err)
	}
	if _, err := app.ReceivedTickets(); err != nil {
		t.Fatalf("listagem de chamados recebidos falhou: %v", err)
	}
}

// Prova de ponta a ponta para o download protegido: o arquivo precisa ser
// recuperado pela sessão curta do computador, sem expor token no WebView. O
// ticket é informado apenas pelo ambiente isolado de homologação.
func TestDownloadAttachmentWithExplicitHomologationConfig(t *testing.T) {
	programData := os.Getenv("BELARC_TEST_PROGRAMDATA")
	ticketCode := os.Getenv("BELARC_TEST_TICKET_WITH_ATTACHMENT")
	if programData == "" || ticketCode == "" {
		t.Skip("configuração/ticket isolado de anexo não definido")
	}
	oldProgramData := os.Getenv("PROGRAMDATA")
	t.Setenv("PROGRAMDATA", programData)
	t.Cleanup(func() { _ = os.Setenv("PROGRAMDATA", oldProgramData) })

	app := NewApp()
	current, err := app.Ticket(ticketCode)
	if err != nil {
		t.Fatalf("não foi possível abrir ticket isolado: %v", err)
	}
	if len(current.Attachments) == 0 {
		t.Fatalf("ticket isolado %s não possui anexo para validar", ticketCode)
	}
	app.mu.Lock()
	path, err := app.downloadAttachmentLocked(current.ID, current.Attachments[0].ID)
	app.mu.Unlock()
	if err != nil {
		t.Fatalf("download protegido falhou: %v", err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(filepath.Dir(path)) })
	info, err := os.Stat(path)
	if err != nil || info.Size() == 0 {
		t.Fatalf("anexo baixado inválido: %v", err)
	}
}
