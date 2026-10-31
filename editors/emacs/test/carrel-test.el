;;; carrel-test.el --- Tests for carrel.el -*- lexical-binding: t; -*-

;; This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.  If a
;; copy of the MPL was not distributed with this file, You can obtain one at
;; https://mozilla.org/MPL/2.0/.

;;; Commentary:

;; Run by editors/test/run.sh, against the fake carrel in editors/test/bin, which records
;; its arguments in $CARREL_FAKE_LOG and prints the fixture JSON in editors/test/fixtures.

;;; Code:

(require 'ert)
(require 'cl-lib)
(require 'carrel)

(defconst carrel-test--root
  (file-name-directory
   (directory-file-name
    (file-name-directory (directory-file-name (file-name-directory
                                               (or load-file-name buffer-file-name))))))
  "The editors folder.")

(defconst carrel-test--archive
  (file-truename (expand-file-name "test/archive/" carrel-test--root))
  "The fixture archive.")

(defconst carrel-test--log (getenv "CARREL_FAKE_LOG")
  "Where the fake carrel records its arguments.")

(defmacro carrel-test--with (&rest body)
  "Run BODY with the fake carrel and an empty call log, then close what BODY opened."
  (declare (indent 0))
  `(let ((carrel-program (expand-file-name "test/bin/carrel" carrel-test--root))
         (carrel-archive nil)
         (carrel-open-function #'find-file)
         (buffers (buffer-list)))
     (with-temp-file carrel-test--log)
     (unwind-protect
         (save-window-excursion ,@body)
       (dolist (buffer (buffer-list))
         (unless (memq buffer buffers)
           (kill-buffer buffer))))))

(defun carrel-test--calls ()
  "The calls the fake carrel recorded, one string each."
  (with-temp-buffer
    (insert-file-contents carrel-test--log)
    (split-string (buffer-string) "\n" t)))

(defun carrel-test--choosing (pick thunk)
  "Call THUNK, answering `completing-read' with the candidate PICK chooses.
PICK is given the candidates' strings, in the order shown, and the
prompt; THUNK's value is returned with the candidates and prompt seen."
  (let (seen prompt-seen)
    (cl-letf (((symbol-function 'completing-read)
               (lambda (prompt table &rest _)
                 (let* ((all (all-completions "" table))
                        (sort (completion-metadata-get
                               (completion-metadata "" table nil)
                               'display-sort-function)))
                   (setq seen (if sort (funcall sort all) all)
                         prompt-seen prompt)
                   (funcall pick seen)))))
      (list (funcall thunk) seen prompt-seen))))

(ert-deftest carrel-test-entries ()
  "Each result says what it is and which file to open."
  (carrel-test--with
    (let* ((results (carrel--run '("search" "furniture")))
           (root (alist-get 'root results))
           (entries (mapcar (lambda (hit) (carrel--entry root hit))
                            (alist-get 'results results)))
           (bill (nth 0 entries)))
      (should (equal (file-name-as-directory (file-truename root)) carrel-test--archive))
      (should (equal (plist-get bill :title) "Furniture shop bill"))
      (should (equal (plist-get bill :mark) "sensitive"))
      (should (equal (plist-get bill :about) "Holding · 2025-03-02 · file"))
      (should (equal (plist-get bill :path) (expand-file-name "Holding/furniture-bill.txt" root)))
      (should (equal (plist-get bill :card) (expand-file-name "Holding/furniture-bill.txt.md" root)))
      (let ((passage (plist-get bill :passage)))
        (should (equal (substring-no-properties passage)
                       "Café chair — please pay invoice 2057-4038 by Friday."))
        ;; The byte ranges come after multibyte characters.
        (should (eq (get-text-property (string-match "invoice" passage) 'face passage)
                    'carrel-match))
        (should (eq (get-text-property (string-match "2057" passage) 'face passage)
                    'carrel-match))
        (should-not (get-text-property (string-match "please" passage) 'face passage)))
      (let ((rfc (nth 1 entries)))
        (should (equal (plist-get rfc :path) (expand-file-name "Reading/RFC 9562 UUIDs.md" root)))
        (should (equal (plist-get rfc :about) "Reading · 2024-05-01 · url · checked out"))
        (should-not (plist-get rfc :mark))
        (should-not (plist-get rfc :passage)))
      (should (equal (plist-get (nth 2 entries) :path)
                     (expand-file-name "Holding/_collection.md" root)))
      (should-not (plist-get (nth 2 entries) :card))
      (should (equal (plist-get (nth 3 entries) :path)
                     (expand-file-name "_System/glossary.md" root))))))

(ert-deftest carrel-test-search-passes-filters-through ()
  "The query's words and filters reach `carrel search' as typed."
  (carrel-test--with
    (carrel-test--choosing #'car
                           (lambda ()
                             (carrel-search "tag:receipts after:2025-01 \"furniture bill\"")))
    (should (equal (carrel-test--calls)
                   '("[--json][search][tag:receipts][after:2025-01][furniture bill]")))))

(ert-deftest carrel-test-search-opens-the-item ()
  "Results are listed in carrel's order, and choosing one opens its file."
  (carrel-test--with
    (pcase-let ((`(,entry ,seen ,prompt)
                 (carrel-test--choosing #'car (lambda () (carrel-search "furniture")))))
      (should (equal prompt "Carrel: furniture bill (4 of 5): "))
      (should (equal (mapcar #'substring-no-properties seen)
                     '("Furniture shop bill [sensitive]  Holding · 2025-03-02 · file  Café chair — please pay invoice 2057-4038 by Friday."
                       "RFC 9562: Universally Unique Identifiers  Reading · 2024-05-01 · url · checked out"
                       "Holding  the collection Holding  Bills and receipts waiting to be filed."
                       "Cowley Road  in the glossary")))
      (should (equal (plist-get entry :title) "Furniture shop bill"))
      (should (equal (file-truename (buffer-file-name))
                     (concat carrel-test--archive "Holding/furniture-bill.txt"))))))

(ert-deftest carrel-test-search-prefix-opens-the-card ()
  "With a prefix argument, choosing a result opens its card."
  (carrel-test--with
    (carrel-test--choosing #'car (lambda () (carrel-search "furniture" t)))
    (should (equal (file-truename (buffer-file-name))
                   (concat carrel-test--archive "Holding/furniture-bill.txt.md")))))

(ert-deftest carrel-test-search-glossary-finds-the-term ()
  "A glossary fact opens the glossary at its term."
  (carrel-test--with
    (carrel-test--choosing (lambda (seen) (nth 3 seen))
                           (lambda () (carrel-search "furniture")))
    (should (equal (file-truename (buffer-file-name))
                   (concat carrel-test--archive "_System/glossary.md")))
    (should (equal (buffer-substring (line-beginning-position) (line-end-position))
                   "## Cowley Road"))))

(ert-deftest carrel-test-search-nothing-found ()
  "Nothing found says so, and asks nothing."
  (carrel-test--with
    (let ((asked nil))
      (cl-letf (((symbol-function 'completing-read) (lambda (&rest _) (setq asked t))))
        (should-not (carrel-search "nothing")))
      (should-not asked))))

(ert-deftest carrel-test-errors ()
  "Carrel's error message is shown."
  (carrel-test--with
    (let ((err (should-error (carrel-search "broken:") :type 'user-error)))
      (should (equal (cadr err) "carrel: the filter broken: isn't one search knows")))
    (let* ((carrel-program "carrel-that-is-not-installed")
           (err (should-error (carrel-search "x") :type 'user-error)))
      (should (string-match-p "isn.t installed" (cadr err))))))

(ert-deftest carrel-test-show ()
  "`carrel-show' shows the current file's card, with keys to open the item and card."
  (carrel-test--with
    (find-file (concat carrel-test--archive "Holding/furniture-bill.txt"))
    (delete-other-windows)
    (carrel-show (buffer-file-name))
    (should (equal (carrel-test--calls)
                   (list (format "[--json][show][%sHolding/furniture-bill.txt]"
                                 carrel-test--archive))))
    (should (derived-mode-p 'carrel-show-mode))
    (should buffer-read-only)
    (should (= (length (window-list)) 2))
    (let ((text (buffer-string)))
      (dolist (want '("Furniture shop bill\n"
                      "  kind         file\n"
                      "  card         Holding/furniture-bill.txt.md\n"
                      "  sensitivity  sensitive\n"
                      "  tags         receipts\n"
                      "  dates        added 2026-10-26 · document 2025-03-02\n"
                      "The bill for a café chair from the furniture shop.\n"
                      "  tags: receipts isn't in the collection's vocabulary\n"
                      "  4f3c2a1b0e  2026-10-26 10:15  file        Filed Furniture shop bill in Holding\n"
                      "o: open the item · c: open the card"))
        (should (string-search want text))))
    (let ((card-window (selected-window)))
      (carrel-show-open-card)
      (should-not (eq (selected-window) card-window))
      (should (equal (file-truename (buffer-file-name))
                     (concat carrel-test--archive "Holding/furniture-bill.txt.md")))
      (select-window card-window)
      (carrel-show-open-item)
      (should (equal (file-truename (buffer-file-name))
                     (concat carrel-test--archive "Holding/furniture-bill.txt")))
      (should (= (length (window-list)) 2)))))

(ert-deftest carrel-test-show-from-dired ()
  "In dired, `carrel-show' shows the card of the file at point."
  (carrel-test--with
    (dired (concat carrel-test--archive "Holding"))
    (goto-char (point-min))
    (search-forward "furniture-bill.txt")
    (should (equal (carrel--file-here)
                   (concat carrel-test--archive "Holding/furniture-bill.txt")))
    (goto-char (point-min))
    (should (equal (carrel--file-here) (concat carrel-test--archive "Holding")))))

(ert-deftest carrel-test-show-outside-the-archive ()
  "A file that isn't in the archive says so."
  (carrel-test--with
    (let ((err (should-error (carrel-show (expand-file-name "test/README.md" carrel-test--root))
                             :type 'user-error)))
      (should (string-match-p "isn't an item or card in this archive" (cadr err))))))

(ert-deftest carrel-test-archive-option ()
  "`carrel-archive' is passed to carrel."
  (carrel-test--with
    (let* ((carrel-archive carrel-test--archive)
           (status (carrel-index-status)))
      (should (equal (alist-get 'behind status) 2))
      (should (equal (carrel-test--calls)
                     (list (format "[--archive][%s][--json][index][status]"
                                   (expand-file-name carrel-test--archive))))))))

(provide 'carrel-test)
;;; carrel-test.el ends here
