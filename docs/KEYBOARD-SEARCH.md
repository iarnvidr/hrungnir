# Search and keyboard navigation

Choose a scope beside the search field: All Reddit, the current subreddit, or Downloaded posts. Submitted downloaded searches run locally and show up to 100 matching posts with title/body/comment snippets. Opening a comment match goes to the cached comment and never falls back to a network fetch.

Public searches send your original words to Reddit, preserving normal partial matching. For episode queries such as `gangs of london s02e08`, Relevance also searches the series name and merges unique results, boosting discussion titles and equivalent episode spellings. This includes titles such as “Gangs of London Season 2 Episode 7-8 discussion” that the old strict Boolean rewrite excluded. Options disables the extra discovery request. Other sorts and quoted/field/Boolean searches remain under Reddit's control. Community buttons above results refine the same query within a community found in those results. These are result refinements, not live community autocomplete. See Reddit's [documented search features](https://support.reddithelp.com/hc/en-us/articles/19696541895316-Available-search-features). Downloaded searches match episode formats automatically and otherwise treat words literally.

| Context | j / k | l | h |
| --- | --- | --- | --- |
| Post or search list | Focus next / previous visible result | Open focused post | Focus current subreddit in the navigator |
| Subreddit navigator | Focus next / previous community | Fetch that community and enter its post list | Remain in navigator |
| Post heading | Focus a comment (or scroll if there are none) | Enter comments | Return to original post list |
| Comment | Focus next / previous visible comment | Expand replies, then enter the first child | Collapse replies, then focus parent or post heading |
| Cached suggestions | Focus next / previous match | Open cached match | Return to the search field |

Arrow keys are alternatives to h/j/k/l: Left/Down/Up/Right respectively. Enter opens focused posts/communities or follows focused links. Cmd+L (Ctrl+L) and `/` focus search; submitting with Enter or the Search button focuses results unless you have moved elsewhere while waiting. Escape leaves the search field while retaining the query and results. Clear explicitly resets search. Escape from the reader returns directly to the list. `?` opens the same shortcut guide available in the header.

Shift+H/K or Shift+Left/Up switches to the previous workspace tab; Shift+J/L or Shift+Down/Right switches to the next. When a subreddit name has focus, these switch communities instead. Plain arrows or hjkl on a post tab also switch tabs. Settings → Tabs lets you choose horizontal or vertical independently for post and subreddit tabs. Both navigators remain present while reading. Library has its own toolbar button.

Navigation shortcuts do not intercept typing, editable content, media controls, or dialogs. Cmd+L deliberately focuses search from any view. Tab/Shift+Tab still reach all controls. Moving list focus alone does not fetch posts. Repeated activation keys are ignored to avoid queuing downloads. Comment collapse state and focus survive redraws within the session.
