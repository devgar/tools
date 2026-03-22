import { getDomains, addDomain, removeDomain } from '@/utils/storage';

const domainList = document.getElementById('domain-list') as HTMLUListElement;
const domainInput = document.getElementById('domain-input') as HTMLInputElement;
const addBtn = document.getElementById('add-btn') as HTMLButtonElement;
const errorMsg = document.getElementById('error-msg') as HTMLParagraphElement;

async function renderDomains(): Promise<void> {
  const domains = await getDomains();
  domainList.innerHTML = '';

  for (const domain of domains) {
    const li = document.createElement('li');

    const span = document.createElement('span');
    span.textContent = domain;
    li.appendChild(span);

    // gitlab.com is not removable
    if (domain !== 'gitlab.com') {
      const removeBtn = document.createElement('button');
      removeBtn.textContent = 'Remove';
      removeBtn.classList.add('remove-btn');
      removeBtn.addEventListener('click', async () => {
        // Revoke host permission
        try {
          await browser.permissions.remove({
            origins: [`*://*.${domain}/*`],
          });
        } catch {
          // Permission might not have been granted
        }

        await removeDomain(domain);
        await renderDomains();
      });
      li.appendChild(removeBtn);
    } else {
      li.classList.add('default');
    }

    domainList.appendChild(li);
  }
}

function showError(msg: string): void {
  errorMsg.textContent = msg;
  errorMsg.hidden = false;
  setTimeout(() => {
    errorMsg.hidden = true;
  }, 3000);
}

addBtn.addEventListener('click', async () => {
  const input = domainInput.value.trim();
  if (!input) {
    showError('Please enter a domain');
    return;
  }

  // Request host permission first
  let granted = false;
  try {
    granted = await browser.permissions.request({
      origins: [`*://*.${input}/*`],
    });
  } catch {
    showError('Failed to request permission');
    return;
  }

  if (!granted) {
    showError('Permission denied');
    return;
  }

  const added = await addDomain(input);
  if (!added) {
    showError('Domain already exists or is invalid');
    return;
  }

  domainInput.value = '';
  await renderDomains();
});

domainInput.addEventListener('keydown', (e) => {
  if (e.key === 'Enter') {
    addBtn.click();
  }
});

// Initial render
renderDomains();
