
        (function () {
            var saved = null;
            try { saved = localStorage.getItem('ui_theme'); } catch (e) {}
            if (saved !== 'light' && saved !== 'dark') {
                saved = window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
            }
            document.documentElement.setAttribute('data-theme', saved);
        })();