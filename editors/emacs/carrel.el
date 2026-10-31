;;; carrel.el --- Search a Carrel archive and show its cards -*- lexical-binding: t; -*-

;; Copyright (C) 2026 Hannah Rose

;; Author: Hannah Rose
;; Maintainer: Hannah Rose
;; Version: 0.3.0
;; Package-Requires: ((emacs "27.1"))
;; Keywords: files, matching
;; URL: https://github.com/HannahARose/carrel

;; This file is not part of GNU Emacs.

;; This Source Code Form is subject to the terms of the Mozilla Public License, v. 2.0.  If a
;; copy of the MPL was not distributed with this file, You can obtain one at
;; https://mozilla.org/MPL/2.0/.

;; SPDX-License-Identifier: MPL-2.0

;;; Commentary:

;; Use a Carrel archive from Emacs.  `carrel-search' searches it by words and meaning, with
;; filters such as tag:receipts or after:2025-01, and lists the results with
;; `completing-read', so it works with vertico, ivy, helm or the default completion.
;; Choosing a result opens its file, or with a prefix argument its card.  `carrel-show'
;; shows the card of the file in the current buffer, or of the file at point in Dired.
;;
;; Nothing is reimplemented here: every command runs the `carrel' program with --json and
;; reads what it prints.  See docs/guide/editors.md in the Carrel repository.

;;; Code:

(require 'json)
(require 'seq)
(require 'subr-x)

(defgroup carrel nil
  "Search a Carrel archive and show its cards."
  :group 'files
  :prefix "carrel-"
  :link '(url-link "https://github.com/HannahARose/carrel"))

(defcustom carrel-program "carrel"
  "The `carrel' program: a name on the variable `exec-path', or a path."
  :type 'string)

(defcustom carrel-archive nil
  "The archive to use, or nil.
When nil, `carrel' finds the archive from `default-directory' (for
`carrel-show', the file's folder), else uses the one chosen with
`carrel open'."
  :type '(choice (const :tag "Found by carrel" nil) directory))

(defcustom carrel-open-function #'find-file
  "The function that opens a result's file, given its absolute path.
Set it to a function that opens files in their own application, such as
`browse-url-of-file', to open items such as PDFs outside Emacs."
  :type 'function)

(defface carrel-match '((t :inherit match))
  "The query's words in a result's passage.")

(defface carrel-title '((t :inherit bold))
  "A result's or card's title.")

(defface carrel-about '((t :inherit shadow))
  "Where and when a result is.")

(defvar carrel-search-history nil
  "Queries given to `carrel-search'.")

(defun carrel--parse (text)
  "Return the JSON value in TEXT, objects as alists, or nil if it isn't JSON."
  (condition-case nil
      (if (and (fboundp 'json-parse-string)
               (or (not (fboundp 'json-available-p)) (json-available-p)))
          (json-parse-string text :object-type 'alist :array-type 'list
                             :null-object nil :false-object nil)
        (let ((json-object-type 'alist) (json-array-type 'list)
              (json-null nil) (json-false nil))
          (json-read-from-string text)))
    (error nil)))

(defun carrel--error (stderr)
  "Return the error carrel printed in STDERR, between its progress lines."
  (or (seq-some (lambda (line)
                  (let ((value (carrel--parse line)))
                    (and (consp value) (alist-get 'error value))))
                (reverse (split-string stderr "\n" t)))
      (let ((text (string-trim stderr)))
        (if (string-empty-p text) "carrel failed" text))))

(defun carrel--run (args &optional directory)
  "Run carrel with --json and ARGS in DIRECTORY, and return the JSON it printed.
DIRECTORY defaults to `default-directory'.  Signal a `user-error' with
carrel's message if it fails."
  (let ((program (executable-find carrel-program)))
    (unless program
      (user-error "%s isn't installed or isn't in `exec-path'; set `carrel-program'"
                  carrel-program))
    (let* ((default-directory (file-name-as-directory
                               (expand-file-name (or directory default-directory))))
           (stderr-file (make-temp-file "carrel-stderr"))
           (args (append (and carrel-archive
                              (list "--archive" (expand-file-name carrel-archive)))
                         (list "--json") args)))
      (unwind-protect
          (with-temp-buffer
            (let* ((coding-system-for-read 'utf-8)
                   (status (apply #'call-process program nil
                                  (list t stderr-file) nil args))
                   (stderr (with-temp-buffer
                             (let ((coding-system-for-read 'utf-8))
                               (insert-file-contents stderr-file))
                             (buffer-string))))
              (unless (eql status 0)
                (user-error "carrel: %s" (carrel--error stderr)))
              (or (carrel--parse (buffer-string))
                  (user-error "Carrel printed something that isn't JSON"))))
        (delete-file stderr-file)))))

(defun carrel--join (root relative)
  "Return RELATIVE, a path in the archive, under ROOT; nil if RELATIVE is nil."
  (and relative (expand-file-name relative root)))

(defun carrel--passage (passage)
  "Return PASSAGE's text with the query's words in the face `carrel-match'.
PASSAGE's highlights are byte ranges into its UTF-8 text."
  (let* ((text (alist-get 'text passage))
         (bytes (encode-coding-string text 'utf-8))
         (at 0)
         (parts nil))
    (dolist (range (alist-get 'highlights passage))
      (let ((start (alist-get 'start range))
            (end (alist-get 'end range)))
        (push (decode-coding-string (substring bytes at start) 'utf-8) parts)
        (push (propertize (decode-coding-string (substring bytes start end) 'utf-8)
                          'face 'carrel-match)
              parts)
        (setq at end)))
    (push (decode-coding-string (substring bytes at) 'utf-8) parts)
    (apply #'concat (nreverse parts))))

(defun carrel--entry (root hit)
  "Return HIT, one of `carrel search''s results under ROOT, as a plist.
It has :title, :about (where and when), :mark (for sensitive and
restricted items), :passage, and the files to open: :path for the
item (its card for a card-only item, a collection's design document,
or the glossary), :card, and :search, text to find in the file."
  (let* ((found (alist-get 'found hit))
         (type (alist-get 'type found))
         (passage (alist-get 'passage hit))
         (entry (list :passage (and passage (carrel--passage passage)))))
    (pcase type
      ("item"
       (let ((date (alist-get 'date found))
             (sensitivity (alist-get 'sensitivity found))
             (card (carrel--join root (alist-get 'card found))))
         (append
          (list :title (alist-get 'title found)
                :about (string-join
                        (delq nil (list (alist-get 'collection found)
                                        (and date (substring date 0 (min 10 (length date))))
                                        (alist-get 'kind found)
                                        (and (alist-get 'checked_out found) "checked out")))
                        " · ")
                :mark (and (member sensitivity '("sensitive" "restricted")) sensitivity)
                :card card
                :path (or (carrel--join root (alist-get 'item found)) card))
          entry)))
      ("collection"
       (append (list :title (alist-get 'title found)
                     :about (concat "the collection " (alist-get 'path found))
                     :path (carrel--join root (concat (alist-get 'path found)
                                                      "/_collection.md")))
               entry))
      (_
       (append (list :title (alist-get 'term found)
                     :about "in the glossary"
                     :path (carrel--join root "_System/glossary.md")
                     :search (alist-get 'term found))
               entry)))))

(defun carrel--line (entry)
  "Return ENTRY, a result from `carrel--entry', on one line."
  (concat (propertize (plist-get entry :title) 'face 'carrel-title)
          (if-let* ((mark (plist-get entry :mark))) (format " [%s]" mark) "")
          "  " (propertize (plist-get entry :about) 'face 'carrel-about)
          (if-let* ((passage (plist-get entry :passage))) (concat "  " passage) "")))

(defun carrel--notice (results)
  "Say in the echo area why RESULTS were found by words only, if they were."
  (let ((meaning (alist-get 'meaning results)))
    (cond ((alist-get 'words_only results)
           (message "carrel: %s" (alist-get 'words_only results)))
          ((equal (alist-get 'state meaning) "behind")
           (message "carrel: the meaning index is behind by %d; they're found by their words until it catches up"
                    (alist-get 'items meaning))))))

(defun carrel--candidates (entries)
  "Return ENTRIES as completion candidates: an alist of unique lines and entries."
  (let ((seen (make-hash-table :test #'equal))
        (candidates nil))
    (dolist (entry entries)
      (let* ((line (carrel--line entry))
             (count (gethash line seen 0)))
        (puthash line (1+ count) seen)
        (push (cons (if (zerop count) line (format "%s <%d>" line (1+ count))) entry)
              candidates)))
    (nreverse candidates)))

(defun carrel--read-result (results)
  "Ask which of RESULTS to open, with `completing-read', and return its entry.
The results stay in carrel's order."
  (let* ((root (alist-get 'root results))
         (candidates (carrel--candidates
                      (mapcar (lambda (hit) (carrel--entry root hit))
                              (alist-get 'results results))))
         (table (lambda (string predicate action)
                  (if (eq action 'metadata)
                      '(metadata (category . carrel-result)
                                 (display-sort-function . identity)
                                 (cycle-sort-function . identity))
                    (complete-with-action action candidates string predicate))))
         (prompt (format "Carrel: %s (%d of %d): " (alist-get 'query results)
                         (length candidates) (alist-get 'total results))))
    (cdr (assoc (completing-read prompt table nil t) candidates))))

(defun carrel-open (entry &optional card)
  "Open ENTRY's file with `carrel-open-function', or its card if CARD is non-nil.
ENTRY is a result as `carrel--entry' makes it.  Entries with no card,
such as collections, open their file."
  (let ((path (or (and card (plist-get entry :card)) (plist-get entry :path))))
    (funcall carrel-open-function path)
    (when-let* ((term (plist-get entry :search)))
      (when (equal (buffer-file-name) path)
        (goto-char (point-min))
        (when (search-forward term nil t)
          (beginning-of-line))))))

(defun carrel--words (query)
  "Split QUERY into the words given to carrel, keeping quoted phrases together."
  (split-string-and-unquote query))

;;;###autoload
(defun carrel-search (query &optional card)
  "Search the Carrel archive for QUERY and open the result chosen.
QUERY is words, filters such as tag:receipts, collection:Finances,
kind:folder, after:2025-01, before:2026 or checked-out, or both, given
to `carrel search' as typed.  The results are listed with
`completing-read', with the passage that matched.  Choosing one opens
its file; with a prefix argument, or CARD non-nil, its card."
  (interactive
   (list (read-string "Search Carrel: " nil 'carrel-search-history)
         current-prefix-arg))
  (when (string-blank-p query)
    (user-error "Search for some words, a filter such as tag:receipts, or both"))
  (let ((results (carrel--run (cons "search" (carrel--words query)))))
    (if (null (alist-get 'results results))
        (progn (message "carrel: nothing found for “%s”" (alist-get 'query results))
               (carrel--notice results)
               nil)
      (carrel--notice results)
      (let ((entry (carrel--read-result results)))
        (when entry
          (carrel-open entry card))
        entry))))

(defvar carrel-show-mode-map
  (let ((map (make-sparse-keymap)))
    (define-key map (kbd "o") #'carrel-show-open-item)
    (define-key map (kbd "c") #'carrel-show-open-card)
    (define-key map (kbd "g") #'carrel-show-refresh)
    map)
  "Keys in a card's buffer, shown by `carrel-show'.")

(define-derived-mode carrel-show-mode special-mode "Carrel card"
  "A card, as `carrel show' gives it.
\\{carrel-show-mode-map}")

(defvar-local carrel-show--shown nil
  "The card this buffer shows, as `carrel show --json' printed it.")

(defvar-local carrel-show--file nil
  "The file whose card this buffer shows.")

(defun carrel--date-row (dates)
  "Return DATES, the card's dates alist, on one line, by name."
  (string-join (sort (mapcar (lambda (date) (format "%s %s" (car date) (cdr date))) dates)
                     #'string<)
               " · "))

(defun carrel--insert-card (shown)
  "Insert SHOWN, the output of `carrel show --json', at point."
  (let* ((fields (alist-get 'fields shown))
         (row (lambda (label value)
                (when (and value (not (equal value "")))
                  (insert (format "  %-13s%s\n" label value)))))
         (checkout (alist-get 'checked_out fields)))
    (insert (propertize (or (alist-get 'title fields) (alist-get 'card shown))
                        'face 'carrel-title)
            "\n\n")
    (funcall row "kind" (alist-get 'kind fields))
    (funcall row "id" (alist-get 'id fields))
    (funcall row "item" (alist-get 'item shown))
    (funcall row "card" (alist-get 'card shown))
    (funcall row "collection" (alist-get 'collection shown))
    (funcall row "sensitivity" (alist-get 'sensitivity fields))
    (funcall row "tags" (string-join (alist-get 'tags fields) ", "))
    (funcall row "dates" (carrel--date-row (alist-get 'dates fields)))
    (funcall row "location" (alist-get 'location fields))
    (funcall row "url" (alist-get 'url fields))
    (when checkout
      (funcall row "checked out" (format "to %s since %s" (alist-get 'to checkout)
                                         (alist-get 'since checkout))))
    (when-let* ((summary (alist-get 'summary fields)))
      (insert "\n" (string-trim summary) "\n"))
    (when-let* ((problems (alist-get 'problems shown)))
      (insert "\nProblems:\n")
      (dolist (problem problems)
        (insert (format "  %s: %s\n" (alist-get 'field problem) (alist-get 'message problem)))))
    (when-let* ((history (alist-get 'history shown)))
      (insert "\nHistory:\n")
      (dolist (change history)
        (let ((time (or (alist-get 'time change) "")))
          (insert (format "  %s  %s  %-11s %s\n"
                          (substring (alist-get 'id change) 0 10)
                          (replace-regexp-in-string
                           "T" " " (substring time 0 (min 16 (length time))))
                          (or (alist-get 'operation change) "")
                          (alist-get 'summary change))))))
    (insert "\n"
            (substitute-command-keys
             "\\<carrel-show-mode-map>\\[carrel-show-open-item]: open the item · \\[carrel-show-open-card]: open the card · \\[quit-window]: close")
            "\n")))

(declare-function dired-get-filename "dired" (&optional localp no-error-if-not-filep))

(defun carrel--file-here ()
  "Return the file of the current buffer, or the file at point in Dired.
In Dired with no file at point, return the folder: it may be an item."
  (cond ((derived-mode-p 'dired-mode)
         (or (dired-get-filename nil t) (directory-file-name default-directory)))
        ((derived-mode-p 'carrel-show-mode) carrel-show--file)
        (t (buffer-file-name))))

;;;###autoload
(defun carrel-show (&optional file)
  "Show the card of FILE, an item or card in a Carrel archive, in another window.
FILE defaults to the current buffer's file, or the file at point in
Dired (the folder itself when there's none).
\\<carrel-show-mode-map>
In the card's buffer, \\[carrel-show-open-item] opens the item and \\[carrel-show-open-card] the card."
  (interactive (list (carrel--file-here)))
  (unless file
    (user-error "This buffer has no file; give `carrel-show' one"))
  (let* ((file (expand-file-name file))
         (directory (if (file-directory-p file) file (file-name-directory file)))
         (shown (carrel--run (list "show" file) directory))
         (title (or (alist-get 'title (alist-get 'fields shown)) (alist-get 'card shown)))
         (buffer (get-buffer-create (format "*carrel: %s*" title))))
    (with-current-buffer buffer
      (let ((inhibit-read-only t))
        (erase-buffer)
        (carrel-show-mode)
        (setq carrel-show--shown shown
              carrel-show--file file
              default-directory (file-name-as-directory (alist-get 'root shown)))
        (carrel--insert-card shown)
        (goto-char (point-min))))
    (pop-to-buffer buffer)
    shown))

(defun carrel-show--open (key)
  "Open the shown card's file under KEY (`item' or `card') in another window."
  (let ((path (carrel--join (alist-get 'root carrel-show--shown)
                            (alist-get key carrel-show--shown))))
    (unless path
      (user-error "This is a card-only item; there's no file to open"))
    (select-window (or (get-mru-window nil nil t) (split-window-sensibly)
                       (split-window)))
    (funcall carrel-open-function path)))

(defun carrel-show-open-item ()
  "Open the item whose card this is, in another window."
  (interactive)
  (carrel-show--open 'item))

(defun carrel-show-open-card ()
  "Open this card's file, in another window."
  (interactive)
  (carrel-show--open 'card))

(defun carrel-show-refresh ()
  "Show the card again, as it is now."
  (interactive)
  (let ((window (selected-window)))
    (carrel-show carrel-show--file)
    (select-window window)))

;;;###autoload
(defun carrel-index-status ()
  "Say how much of the Carrel archive the meaning index covers."
  (interactive)
  (let ((status (carrel--run (list "index" "status"))))
    (message "%s"
             (cond ((null (alist-get 'model status))
                    "carrel: no embedding model yet, so search uses words only")
                   ((null (alist-get 'exists status))
                    (format "carrel: no meaning index for %s yet; run carrel index rebuild"
                            (alist-get 'model status)))
                   (t (format "carrel: %s: %d of %d items embedded, %d behind"
                              (alist-get 'model status) (alist-get 'embedded status)
                              (alist-get 'items status) (alist-get 'behind status)))))
    status))

(provide 'carrel)
;;; carrel.el ends here
