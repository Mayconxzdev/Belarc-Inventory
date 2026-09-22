package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"sync"
	"time"

	toast "git.sr.ht/~jackmordaunt/go-toast/v2"
	"github.com/BurntSushi/toml"
)

const maxAttachmentBytes = 8 * 1024 * 1024
const maxAttachmentDownloadBytes = maxAttachmentBytes + 64*1024

type agentConfig struct {
	ServerURL  string `toml:"server_url"`
	AgentToken string `toml:"agent_token"`
}

type portalSession struct {
	Token              string   `json:"token"`
	ReceiveDepartments []string `json:"receive_departments"`
	ExpiresAt          string   `json:"expires_at"`
}

type routingInfo struct {
	ReceiveDepartments []string `json:"receive_departments"`
}

type department struct {
	Slug   string `json:"slug"`
	Name   string `json:"name"`
	Active bool   `json:"active"`
}

type machine struct {
	ID       string `json:"id"`
	Hostname string `json:"hostname"`
}

type ticket struct {
	ID                string       `json:"id"`
	Code              string       `json:"code"`
	MachineID         string       `json:"machine_id"`
	HostnameSnapshot  string       `json:"hostname_snapshot"`
	OwnerNameSnapshot string       `json:"owner_name_snapshot"`
	Title             string       `json:"title"`
	Description       string       `json:"description"`
	Status            string       `json:"status"`
	Priority          string       `json:"priority"`
	Department        string       `json:"department"`
	CreatedAt         string       `json:"created_at"`
	UpdatedAt         string       `json:"updated_at"`
	Resolution        string       `json:"resolution"`
	Attachments       []attachment `json:"attachments"`
	Comments          []comment    `json:"comments"`
}

type attachment struct {
	ID        string `json:"id"`
	Filename  string `json:"filename"`
	CreatedAt string `json:"created_at"`
}

type comment struct {
	ID         string `json:"id"`
	AuthorRole string `json:"author_role"`
	AuthorName string `json:"author_name"`
	Body       string `json:"body"`
	CreatedAt  string `json:"created_at"`
}

type attachmentInput struct {
	Filename      string `json:"filename"`
	ContentBase64 string `json:"content_base64"`
}

type createTicketInput struct {
	Title       string            `json:"title"`
	Description string            `json:"description"`
	Priority    string            `json:"priority"`
	Department  string            `json:"department"`
	Attachments []attachmentInput `json:"attachments"`
}

type bootInfo struct {
	Connected          bool         `json:"connected"`
	Hostname           string       `json:"hostname"`
	ReceiveDepartments []string     `json:"receive_departments"`
	Departments        []department `json:"departments"`
	Message            string       `json:"message,omitempty"`
}

// App is the local bridge. The browser-like frontend never receives the long-lived
// agent token; it receives data only through these native methods.
type App struct {
	ctx     context.Context
	mu      sync.Mutex
	http    *http.Client
	config  agentConfig
	session portalSession
	machine machine
}

func NewApp() *App {
	return &App{http: &http.Client{Timeout: 20 * time.Second}}
}

func (a *App) startup(ctx context.Context)  { a.ctx = ctx }
func (a *App) shutdown(ctx context.Context) {}

func configPath() string {
	base := os.Getenv("PROGRAMDATA")
	if strings.TrimSpace(base) == "" {
		base = `C:\ProgramData`
	}
	return filepath.Join(base, "BelarcInventory", "config.toml")
}

func (a *App) Bootstrap() bootInfo {
	a.mu.Lock()
	defer a.mu.Unlock()
	if err := a.connectLocked(); err != nil {
		return bootInfo{Connected: false, Message: userError(err)}
	}
	departments, err := a.departmentsLocked()
	if err != nil {
		return bootInfo{Connected: false, Message: userError(err)}
	}
	return bootInfo{Connected: true, Hostname: a.machine.Hostname, ReceiveDepartments: a.session.ReceiveDepartments, Departments: departments}
}

// RefreshRouting sincroniza somente os setores atendidos por este computador.
// O token permanente do agente permanece no processo nativo e a sessão curta
// existente continua sendo usada para os chamados.
func (a *App) RefreshRouting() bootInfo {
	a.mu.Lock()
	defer a.mu.Unlock()
	if err := a.connectLocked(); err != nil {
		return bootInfo{Connected: false, Message: userError(err)}
	}
	var routing routingInfo
	if err := a.agentJSONLocked(http.MethodGet, "/api/portal/device-routing", nil, &routing); err != nil {
		return bootInfo{Connected: false, Message: userError(err)}
	}
	a.session.ReceiveDepartments = routing.ReceiveDepartments
	departments, err := a.departmentsLocked()
	if err != nil {
		return bootInfo{Connected: false, Message: userError(err)}
	}
	return bootInfo{Connected: true, Hostname: a.machine.Hostname, ReceiveDepartments: routing.ReceiveDepartments, Departments: departments}
}

func (a *App) MyTickets() ([]ticket, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if err := a.connectLocked(); err != nil {
		return nil, err
	}
	var result []ticket
	return result, a.getJSONLocked("/api/tickets/mine", &result)
}

func (a *App) ReceivedTickets() ([]ticket, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if err := a.connectLocked(); err != nil {
		return nil, err
	}
	if len(a.session.ReceiveDepartments) == 0 {
		return []ticket{}, nil
	}
	var result []ticket
	return result, a.getJSONLocked("/api/tickets", &result)
}

func (a *App) Ticket(id string) (ticket, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if err := a.connectLocked(); err != nil {
		return ticket{}, err
	}
	var result ticket
	return result, a.getJSONLocked("/api/tickets/"+id, &result)
}

func (a *App) CreateTicket(input createTicketInput) (ticket, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if err := a.connectLocked(); err != nil {
		return ticket{}, err
	}
	input.Title = strings.TrimSpace(input.Title)
	input.Description = strings.TrimSpace(input.Description)
	if input.Title == "" {
		return ticket{}, errors.New("Informe o título do chamado.")
	}
	if input.Department == "" {
		return ticket{}, errors.New("Escolha o setor que receberá o chamado.")
	}
	for _, file := range input.Attachments {
		if len(file.ContentBase64) > maxAttachmentBytes*2 {
			return ticket{}, fmt.Errorf("o arquivo %q ultrapassa o limite de 8 MB", file.Filename)
		}
	}
	payload := map[string]any{"machine_id": a.machine.ID, "title": input.Title, "description": input.Description, "priority": input.Priority, "department": input.Department, "attachments": input.Attachments}
	var result ticket
	return result, a.sendJSONLocked(http.MethodPost, "/api/tickets", payload, &result)
}

func (a *App) UpdateTicket(id string, status string, resolution string, attachments []attachmentInput) (ticket, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if err := a.connectLocked(); err != nil {
		return ticket{}, err
	}
	var result ticket
	if status == "done" {
		return result, a.sendJSONLocked(http.MethodPost, "/api/tickets/"+id+"/close", map[string]any{"resolution": strings.TrimSpace(resolution), "attachments": attachments}, &result)
	}
	return result, a.sendJSONLocked(http.MethodPatch, "/api/tickets/"+id, map[string]any{"status": status}, &result)
}

func (a *App) AddComment(id string, body string, attachments []attachmentInput) (ticket, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if err := a.connectLocked(); err != nil {
		return ticket{}, err
	}
	if strings.TrimSpace(body) == "" && len(attachments) == 0 {
		return ticket{}, errors.New("Escreva uma mensagem ou anexe um arquivo antes de enviar.")
	}
	var result ticket
	return result, a.sendJSONLocked(http.MethodPost, "/api/tickets/"+id+"/comments", map[string]any{"body": strings.TrimSpace(body), "attachments": attachments}, &result)
}

// OpenAttachment baixa um anexo autorizado usando a sessão curta do portal e o
// abre pelo aplicativo padrão do Windows. O token nunca é entregue ao WebView.
// O arquivo é salvo em uma pasta temporária exclusiva do processo para que o
// navegador não precise acessar uma URL autenticada diretamente.
func (a *App) OpenAttachment(ticketID string, attachmentID string) error {
	a.mu.Lock()
	defer a.mu.Unlock()
	path, err := a.downloadAttachmentLocked(ticketID, attachmentID)
	if err != nil {
		return err
	}
	if err := exec.Command("explorer.exe", path).Start(); err != nil {
		return fmt.Errorf("não foi possível abrir o anexo: %w", err)
	}
	return nil
}

// downloadAttachmentLocked é separado da abertura para permitir validação de
// integração sem iniciar aplicativos externos durante os testes.
func (a *App) downloadAttachmentLocked(ticketID string, attachmentID string) (string, error) {
	if strings.TrimSpace(ticketID) == "" || strings.TrimSpace(attachmentID) == "" {
		return "", errors.New("anexo inválido")
	}
	if err := a.connectLocked(); err != nil {
		return "", err
	}
	var current ticket
	if err := a.getJSONLocked("/api/tickets/"+ticketID, &current); err != nil {
		return "", err
	}
	filename := "anexo"
	for _, file := range current.Attachments {
		if file.ID == attachmentID {
			filename = safeFilename(file.Filename)
			break
		}
	}
	if filename == "anexo" {
		return "", errors.New("anexo não encontrado neste chamado")
	}
	req, err := http.NewRequest(http.MethodGet, a.config.ServerURL+"/api/tickets/"+ticketID+"/attachments/"+attachmentID, nil)
	if err != nil {
		return "", err
	}
	req.Header.Set("Authorization", "Bearer "+a.session.Token)
	resp, err := a.http.Do(req)
	if err != nil {
		return "", fmt.Errorf("não foi possível baixar o anexo: %w", err)
	}
	defer resp.Body.Close()
	bytes, err := io.ReadAll(io.LimitReader(resp.Body, maxAttachmentDownloadBytes+1))
	if err != nil {
		return "", fmt.Errorf("não foi possível ler o anexo: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		message := strings.TrimSpace(string(bytes))
		if message == "" {
			message = resp.Status
		}
		return "", fmt.Errorf("servidor recusou o download (%d): %s", resp.StatusCode, message)
	}
	if len(bytes) > maxAttachmentDownloadBytes {
		return "", errors.New("o anexo ultrapassa o limite permitido de 8 MB")
	}
	dir, err := os.MkdirTemp("", "BelarcChamados-")
	if err != nil {
		return "", fmt.Errorf("não foi possível preparar o anexo: %w", err)
	}
	path := filepath.Join(dir, filename)
	if err := os.WriteFile(path, bytes, 0o600); err != nil {
		return "", fmt.Errorf("não foi possível salvar o anexo: %w", err)
	}
	return path, nil
}

func safeFilename(value string) string {
	name := filepath.Base(strings.TrimSpace(value))
	name = strings.Map(func(r rune) rune {
		switch r {
		case '<', '>', ':', '"', '/', '\\', '|', '?', '*':
			return '_'
		default:
			return r
		}
	}, name)
	if name == "" || name == "." || name == ".." {
		return "anexo"
	}
	return name
}

func (a *App) Notify(title string, message string) error {
	notification := toast.Notification{
		AppID: "Belarc.ChamadosServicosTI",
		Title: strings.TrimSpace(title),
		Body:  strings.TrimSpace(message),
	}
	return notification.Push()
}

func (a *App) connectLocked() error {
	if a.session.Token != "" && a.sessionValidLocked() {
		return nil
	}
	var cfg agentConfig
	path := configPath()
	if _, err := toml.DecodeFile(path, &cfg); err != nil {
		return fmt.Errorf("não foi possível ler a configuração do agente em %s: %w", path, err)
	}
	cfg.ServerURL = strings.TrimRight(strings.TrimSpace(cfg.ServerURL), "/")
	if cfg.ServerURL == "" || strings.TrimSpace(cfg.AgentToken) == "" {
		return errors.New("o agente deste PC ainda não está configurado para o servidor de chamados")
	}
	a.config = cfg
	var session portalSession
	if err := a.agentJSONLocked(http.MethodPost, "/api/portal/device-session", map[string]string{"mode": "desktop"}, &session); err != nil {
		return err
	}
	if session.Token == "" {
		return errors.New("o servidor não retornou uma sessão válida para este computador")
	}
	a.session = session
	var machines []machine
	if err := a.getJSONLocked("/api/portal/machines", &machines); err != nil {
		return err
	}
	if len(machines) != 1 {
		return errors.New("não foi possível identificar exclusivamente este computador no inventário")
	}
	a.machine = machines[0]
	return nil
}

func (a *App) sessionValidLocked() bool {
	if a.session.ExpiresAt == "" {
		return false
	}
	expires, err := time.Parse(time.RFC3339, a.session.ExpiresAt)
	return err == nil && time.Until(expires) > 5*time.Minute
}

func (a *App) departmentsLocked() ([]department, error) {
	var result []department
	err := a.getJSONLocked("/api/ticket-departments", &result)
	return result, err
}

func (a *App) getJSONLocked(path string, target any) error {
	return a.requestJSONLocked(http.MethodGet, path, nil, a.session.Token, target)
}

func (a *App) agentJSONLocked(method, path string, body any, target any) error {
	return a.requestJSONLocked(method, path, body, a.config.AgentToken, target)
}

func (a *App) sendJSONLocked(method, path string, body any, target any) error {
	return a.requestJSONLocked(method, path, body, a.session.Token, target)
}

func (a *App) requestJSONLocked(method, path string, body any, bearer string, target any) error {
	var reader io.Reader
	if body != nil {
		encoded, err := json.Marshal(body)
		if err != nil {
			return err
		}
		reader = bytes.NewReader(encoded)
	}
	req, err := http.NewRequest(method, a.config.ServerURL+path, reader)
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Bearer "+bearer)
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	resp, err := a.http.Do(req)
	if err != nil {
		return fmt.Errorf("não foi possível conectar ao servidor: %w", err)
	}
	defer resp.Body.Close()
	data, _ := io.ReadAll(io.LimitReader(resp.Body, 2*1024*1024))
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		message := strings.TrimSpace(string(data))
		if message == "" {
			message = resp.Status
		}
		return fmt.Errorf("servidor recusou a solicitação (%d): %s", resp.StatusCode, message)
	}
	if target != nil && len(data) > 0 {
		return json.Unmarshal(data, target)
	}
	return nil
}

func userError(err error) string {
	if err == nil {
		return ""
	}
	return err.Error()
}
