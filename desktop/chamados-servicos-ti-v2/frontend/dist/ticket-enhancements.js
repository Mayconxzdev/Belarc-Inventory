// Complementos do cliente de chamados. Mantido separado para preservar a tela
// homologada e permitir evoluir anexos sem expor token do agente ao WebView.
(() => {
  'use strict';
  let selectedId = '';
  let selectedTicket = null;
  let wrapped = false;
  const revisions = new Map();
  const filesFor = new WeakMap();
  const max = 8 * 1024 * 1024;
  const esc = value => String(value || '').replace(/[&<>'"]/g, c => ({'&':'&amp;','>':'&gt;','<':'&lt;',"'":'&#39;','"':'&quot;'}[c]));
  const friendlyName = file => {
    const name = String(file.name || 'anexo');
    if (/^\.?\{?[0-9a-f]{8}[-_][0-9a-f-]{20,}\}?\.(png|jpe?g|webp|bmp)$/i.test(name)) {
      const ext = (name.split('.').pop() || 'png').toLowerCase();
      return `captura-de-tela-${new Date().toISOString().replace(/[-:TZ.]/g,'').slice(0,14)}.${ext}`;
    }
    return name;
  };
  const toPayload = file => new Promise((resolve, reject) => {
    if (file.size > max) return reject(new Error(`${file.name} ultrapassa 8 MB.`));
    const reader = new FileReader();
    reader.onerror = () => reject(new Error(`Não foi possível ler ${file.name}.`));
    reader.onload = () => resolve({filename:friendlyName(file), content_base64:String(reader.result).split(',')[1] || ''});
    reader.readAsDataURL(file);
  });
  function native() { return window.go && window.go.main && window.go.main.App; }
  function renderTicketFiles() {
    const card = document.querySelector('.detail-card');
    if (!card || !selectedTicket) return;
    const attachments = selectedTicket.attachments || [];
    let section = card.querySelector('.ticket-files');
    if (!attachments.length) { section?.remove(); return; }
    if (!section) { section = document.createElement('section'); section.className = 'ticket-files timeline'; card.appendChild(section); }
    section.innerHTML = `<h3>Anexos do chamado</h3><div class="file-list">${attachments.map(file => `<div class="ticket-file"><span>📎 ${esc(file.filename)} <span class="muted">· ${esc(file.created_at || '')}</span></span><button type="button" data-open-attachment="${esc(file.id)}">Abrir</button></div>`).join('')}</div>`;
    section.querySelectorAll('[data-open-attachment]').forEach(button => button.onclick = async () => {
      try { button.disabled = true; await native().OpenAttachment(selectedId, button.dataset.openAttachment); }
      catch (err) { alert(err.message || err); }
      finally { button.disabled = false; }
    });
  }
  function renderSelected(form) {
    const list = filesFor.get(form) || [];
    const out = form.querySelector('.inline-file-list');
    if (!out) return;
    out.innerHTML = list.map((file, index) => `<span class="attachment-chip" title="${esc(file.name)}"><span class="attachment-chip-name">📎 ${esc(friendlyName(file))}</span><span class="attachment-chip-size">${Math.ceil(file.size/1024)} KB</span><button type="button" aria-label="Remover ${esc(friendlyName(file))}" data-remove-file="${index}">×</button></span>`).join('');
    out.querySelectorAll('[data-remove-file]').forEach(button => button.onclick = () => {
      const current = filesFor.get(form) || [];
      current.splice(Number(button.dataset.removeFile), 1);
      filesFor.set(form, current);
      renderSelected(form);
    });
  }
  function addFiles(form, incoming) {
    const list = filesFor.get(form) || [];
    for (const file of [...incoming || []]) {
      if (!list.some(old => old.name === file.name && old.size === file.size && old.lastModified === file.lastModified)) list.push(file);
    }
    filesFor.set(form, list);
    renderSelected(form);
  }
  function attachPicker(form, label) {
    if (form.querySelector('.attachment-tools')) return;
    const tools = document.createElement('div');
    tools.className = 'attachment-tools';
    tools.innerHTML = `<input type="file" multiple hidden><button type="button" class="attachment-button">📎 ${label}</button><span class="attachment-hint">ou arraste arquivos / cole uma imagem</span><div class="inline-file-list" aria-live="polite"></div>`;
    const input = tools.querySelector('input');
    tools.querySelector('button').onclick = () => input.click();
    input.onchange = () => addFiles(form, input.files);
    form.appendChild(tools);
    form.addEventListener('dragover', e => { e.preventDefault(); tools.classList.add('is-dragging'); });
    form.addEventListener('dragleave', () => tools.classList.remove('is-dragging'));
    form.addEventListener('drop', e => { e.preventDefault(); tools.classList.remove('is-dragging'); addFiles(form, e.dataTransfer.files); });
  }
  function install() {
    const api = native();
    if (!api) return setTimeout(install, 250);
    if (!wrapped) {
      const ticket = api.Ticket.bind(api);
      api.Ticket = async id => { selectedId = id; selectedTicket = await ticket(id); return selectedTicket; };
      for (const method of ['MyTickets', 'ReceivedTickets']) {
        const original = api[method].bind(api);
        api[method] = async () => {
          const list = await original();
          for (const item of list || []) {
            const revision = `${item.updated_at || ''}|${item.status || ''}|${(item.comments || []).length}|${(item.attachments || []).length}`;
            const previous = revisions.get(`${method}:${item.id}`);
            if (previous && previous !== revision) {
              const action = item.status === 'done' ? 'Chamado concluído' : method === 'ReceivedTickets' ? 'Chamado atualizado' : 'Resposta no chamado';
              api.Notify(action, `${item.code} — ${item.title}`).catch(() => {});
            }
            revisions.set(`${method}:${item.id}`, revision);
          }
          return list;
        };
      }
      wrapped = true;
    }
    renderTicketFiles();
    const comment = document.querySelector('#comment-form');
    if (comment && !comment.dataset.attachmentsReady) {
      comment.dataset.attachmentsReady = '1'; attachPicker(comment, 'Adicionar anexo');
      const text = comment.querySelector('#comment-body'); if (text) { text.lang='pt-BR'; text.spellcheck=true; }
      comment.onsubmit = async e => { e.preventDefault(); const button=[...comment.querySelectorAll('button')].find(b=>b.textContent.trim()==='Enviar'); try { if (!selectedId) throw new Error('Abra novamente a ficha do chamado.'); button.disabled=true; const attachments=await Promise.all((filesFor.get(comment)||[]).map(toPayload)); const updated=await native().AddComment(selectedId, text.value, attachments); selectedTicket=updated; const box=document.querySelector('#comments'); if(box){ const last=updated.comments[updated.comments.length-1]; box.insertAdjacentHTML('beforeend',`<article class="comment"><strong>${esc(last.author_name||last.author_role||'Usuário')}</strong><small> · agora</small><p>${esc(last.body)}</p>${attachments.map(a=>`<p class="muted">📎 ${esc(a.filename)}</p>`).join('')}</article>`); } text.value=''; filesFor.set(comment,[]); addFiles(comment,[]); renderTicketFiles(); } catch(err){ alert(err.message||err); } finally { button.disabled=false; } };
    }
    const save = document.querySelector('#save-status');
    if (save && !save.dataset.attachmentsReady) {
      save.dataset.attachmentsReady='1'; const field=save.parentElement; attachPicker(field, 'Adicionar anexo à resolução'); const resolution=document.querySelector('#resolution'); if(resolution){ resolution.lang='pt-BR'; resolution.spellcheck=true; }
      save.onclick=async()=>{ try { if(!selectedId) throw new Error('Abra novamente a ficha do chamado.'); save.disabled=true; const attachments=await Promise.all((filesFor.get(field)||[]).map(toPayload)); selectedTicket=await native().UpdateTicket(selectedId, document.querySelector('#status').value, resolution.value, attachments); save.textContent='Andamento salvo'; filesFor.set(field,[]); renderTicketFiles(); } catch(err){ alert(err.message||err); } finally { save.disabled=false; } };
    }
  }
  new MutationObserver(install).observe(document.body, {childList:true, subtree:true});
  document.addEventListener('paste', event => { const form=document.querySelector('#comment-form'); const field=document.querySelector('#save-status')?.parentElement; const target=event.target; const container=target?.closest('#comment-form') || (target?.closest('.detail-card') && field); const images=[...(event.clipboardData?.files||[])].filter(f=>f.type.startsWith('image/')); if(container && images.length){event.preventDefault(); addFiles(container,images);} });
  install();
})();
