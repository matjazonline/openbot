"use strict";
for (const button of document.querySelectorAll(".event-retry")) {
  button.addEventListener("click", async () => {
    if (button.disabled) return;
    button.disabled = true;
    const status = document.getElementById("report-status");
    status.textContent = "Retrying this event…";
    try {
      // Never attach report-read cancellation to a durable write. Preserve the
      // original operation identity if its response is lost.
      const response = await fetch("/v1/merchant-events/" + encodeURIComponent(button.dataset.event) + "/retry", {
        method: "POST", headers: {"X-Company-ID":button.dataset.company,
          "X-CSRF-Token":button.dataset.csrf,"Idempotency-Key":button.dataset.key,
          "Idempotency-Created-At":button.dataset.created}
      });
      if (!response.ok) throw new Error("Retry failed (" + response.status + "). You can retry the same operation.");
      const result = await response.json();
      status.textContent = "Event state: " + result.state + ". Refresh this page to see current attempts.";
      button.textContent = "Retry recorded";
    } catch (error) {
      status.textContent = error.message;
      button.disabled = false;
    }
  });
}
