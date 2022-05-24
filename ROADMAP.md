# Roadmap

## alpha-version

 - [X] Implement query handler
 - [X] Download of binary files
 - [X] Refactor: Get rid of server/port/path - use url::URL
 - [X] Error handling for URLs
 - [X] Refactor: Move non-ui functions to controller
 - [X] Refactor: get rid of contenttype, use itemtype
 - [X] Implement open history entries [6/6]
   - [X] Open URL from history-menu
   - [X] Add count to history
   - [X] Handle duplicate history entries (not nesassary)
   - [X] Implement show all history dialog
   - [X] Write history to file
   - [X] Read history from file on startup

 - [X] Implement simple bookmark handling [2/2]
    Bookmark: Name, Url, Last visited, Tags?

   - [X] Write bookmarks to file
   - [X] Read bookmark from file on startup

 - [X] Implement settings dialog [4/5]
   - [X] Set homepage
   - [X] Default directory for downloads
   - [X] Search engines (maybe later?)
   - [X] Read config from file (<https://github.com/mehcode/config-rs>)
   - [X] Write configuration to file

How to handle config-file changes? Overwrite existing config-file?
Possible solution: create config-auto, and use config to extend
config-auto Ignore config-filechanges while running for now

 - [X] [#B] Implement download of files (text/gophermap)
 - [X] [#C] Write README.org

## post-alpha
----------

 - [X] [#A] Bugfix: Prohibit duplicate bookmark entries, open existing entry
 - [X] [#A] Bugfix: Reload must not add current page to history
 - [ ] Configurable keys
 - [X] Better keyboard navigation, emacs/vim key presets
 - [X] SPACE to page
 - [X] Settings dialog
 - [X] Setting for disabling history recording
 - [X] Setting for text wrap column
 - [ ] Tor support for gopher
 - [ ] Handle tags for bookmarks
 - [X] Search in text
 - [ ] Caching of gophermaps
 - [ ] mailcap handling
 - [ ] Reading list (ala Safari)
 - [ ] Bookmarks [0/1]
   - [ ] Export bookmarks to gophermap/gemini-txt/txt
 - [X] [#C] Themes
 - [X] [#C] Add tracing of UiMessage and ControllerMessage in log
 - [X] [#A] Bugfix: search not working
 - [X] TLS support
 - [X] Write man page
 - [X] Persistent history
 - [X] Show info about link under cursor
 - [X] Implement reload of page

## beta, etc

 - Gemini support [6/9]
   - [X] Binary downloads
   - [X] Automatic text wrapping
   - [X] Handle prefomatting toggle lines
   - [X] Bugfix: Can\'t open WWW links from gemini
   - [X] Implement save as text for gemini
   - [X] Limit number of redirects to 5
   - [ ] Warning when redirecting to external server
   - [ ] Client certificates, see [Alex\' gemini wiki](https://alexschroeder.ch/wiki/2020-07-13_Client_Certificates_and_IO%3a%3aSocket%3a%3aSSL_(Perl))
   - [X] TOFU certificate pinning

 - [ ] Open local file (gophermap/textfile)
 - [ ] Auto moka pona (rss-like?), maybe rss support
 - [ ] Download gopherhole for offline reading
 - [ ] Setting for encoding
