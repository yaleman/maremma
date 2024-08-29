function confirmForm(elementId, confirmText) {
    document.addEventListener('DOMContentLoaded', function() {
        const form = document.getElementById(elementId);
        if (form) {
            form.addEventListener('submit', function(event) {
                event.preventDefault();
                if (confirm(confirmText)) {
                    this.submit();
                }
            });
        }
    });
};