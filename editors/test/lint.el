;;; lint.el --- Checks for carrel.el -*- lexical-binding: t; -*-

;;; Commentary:

;; Run by run.sh: byte-compiles carrel.el with warnings as errors, then runs checkdoc and,
;; when it's installed (Debian and Ubuntu's elpa-package-lint), package-lint.

;;; Code:

(require 'bytecomp)
(require 'checkdoc)

(defun carrel-test-lint (file compiled)
  "Byte-compile FILE to COMPILED, then run checkdoc and package-lint on FILE.
Exit with a failure if any of them complains."
  (let ((byte-compile-error-on-warn t)
        (byte-compile-dest-file-function (lambda (_) compiled))
        (failed nil))
    (unless (byte-compile-file file)
      (setq failed t))
    (message "byte-compile: %s" (if failed "failed" "clean"))
    (with-current-buffer (find-file-noselect file)
      (let ((checkdoc-diagnostic-buffer "*checkdoc*")
            (checkdoc-autofix-flag 'never)
            (checkdoc-spellcheck-documentation-flag nil))
        (checkdoc-current-buffer t)
        (with-current-buffer (get-buffer-create checkdoc-diagnostic-buffer)
          (let ((found (seq-filter (lambda (line) (string-match-p "carrel\\.el:[0-9]+" line))
                                   (split-string (buffer-string) "\n" t))))
            (dolist (line found) (message "checkdoc: %s" line))
            (message "checkdoc: %s" (if found "failed" "clean"))
            (when found (setq failed t)))))
      (dolist (dir (file-expand-wildcards "/usr/share/emacs/site-lisp/elpa/*"))
        (add-to-list 'load-path dir))
      (if (not (require 'package-lint nil t))
          (message "package-lint: not installed, skipped")
        (let ((problems (package-lint-buffer)))
          (dolist (problem problems)
            (message "package-lint: %s:%d:%d: %s: %s" file (nth 0 problem) (nth 1 problem)
                     (nth 2 problem) (nth 3 problem)))
          (message "package-lint: %s" (if problems "failed" "clean"))
          (when problems (setq failed t)))))
    (kill-emacs (if failed 1 0))))

;;; lint.el ends here
