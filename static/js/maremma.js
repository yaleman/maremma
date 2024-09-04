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

function resetSearch(formElementId, searchElementId) {
    document.addEventListener('DOMContentLoaded', function() {
        const form = document.getElementById(formElementId);
        if (form) {
            form.addEventListener('reset', function() {
                let searchElement = document.getElementById(searchElementId);
                searchElement.value = "";

                this.submit();
            });
        }
    });
}