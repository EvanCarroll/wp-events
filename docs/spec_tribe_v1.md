# The Events Calendar — legacy REST API `tribe/events/v1` — wire-level specification

Source: the upstream [the-events-calendar](https://github.com/the-events-calendar/the-events-calendar) repository (TEC **6.18.3.1**, `the-events-calendar.php:5`), plus the `common/` submodule.
All `file:line` references are relative to that directory. `src/Tribe/REST/V1/` is abbreviated `V1/`.

WordPress core behaviour (WP_REST_Server / WP_REST_Request / WP_REST_Terms_Controller) is **not in this repo**. Where the spec depends on it, it is marked **[WP core]** and based on WP ≥ 5.7 behaviour. Verify those parts against a live site if they matter.

Conventions used below:
- `int` = JSON number (integer). `str` = JSON string. `bool` = JSON true/false.
- "**digits-string**" = a JSON string holding a decimal integer, e.g. `"1"`.
- "**DB datetime**" = string in `Y-m-d H:i:s` format, e.g. `"2024-05-01 19:00:00"`, no timezone marker.
- "absent" = key not present. "null" = key present with JSON `null`.
- PHP empty array `[]` always encodes as JSON `[]`, even where a non-empty value of the same field would be a JSON **object**. This matters a lot below.

---------------------------------------------------------------------------------------------------

## 0. Base URL, namespace, versioning

| Item | Value | Source |
|---|---|---|
| Namespace | `tribe/events/v1` | `common/src/Tribe/REST/Main.php:18` (`tribe`), `V1/Main.php:20` (`/events/v1`), `V1/Main.php:269-280` |
| Pretty-permalink base | `{home}/wp-json/tribe/events/v1/` (the `wp-json` prefix is WP's `rest_get_url_prefix()`, which can be changed by the `rest_url_prefix` filter) | `common/src/Tribe/REST/Main.php:34-53` |
| Non-pretty-permalink base | `{home}/index.php?rest_route=/tribe/events/v1/...` (or `?rest_route=` on the home URL) **[WP core]** `get_rest_url()` | `common/src/Tribe/REST/Main.php:68-111` |
| Semantic API version (Swagger `info.version`) | `"1.0.0"` | `V1/Main.php:260-262` |
| Version string in headers | `"v1"` | `V1/Main.php:278-280` |

All URLs the API returns (`rest_url`, `next_rest_url`, `urls.self`, ...) come from `tribe_events_rest_url()` → `get_rest_url()` (`src/functions/advanced-functions/rest-v1.php:27-29`). They can be either form above, and the filter `tribe_rest_url` (`common/src/Tribe/REST/Main.php:110`) can rewrite them. **Treat every returned URL as opaque and absolute.**

---------------------------------------------------------------------------------------------------

## 1. Global enable/disable switches

### 1.1 TEC-level switch
`V1/System.php:33-40`:
```php
$enabled = $this->supports_tec_rest_api() && false == tribe_get_option( 'rest-v1-disabled' );
return apply_filters( 'tribe_events_rest_api_enabled', $enabled );
```
- `supports_tec_rest_api()` is true when `function_exists('get_rest_url')` (WP ≥ 4.4) (`common/src/Tribe/REST/System.php:10-12`).
- The option `rest-v1-disabled` lives in the TEC options array (`tribe_get_option`). In this version no settings-UI field is added for it (`V1/Settings.php:42-44` returns the fields unchanged). It can only be set programmatically. The `tribe_events_rest_api_enabled` filter can also force the value either way.
- If the API is disabled, `V1/Main.php:58-60` returns early. **No routes are registered.** `rest_api_init` is never hooked.

**What a client sees when the API is disabled [WP core]:** every request to `/wp-json/tribe/events/v1/...`, including `/doc` and the namespace index `/wp-json/tribe/events/v1`, returns
```json
HTTP 404
{"code":"rest_no_route","message":"No route was found matching the URL and request method.","data":{"status":404}}
```
Front-end HTML pages then also send the response header `X-TEC-API-VERSION: disabled` (§5.2).

If the WP REST API itself is unavailable (pre-4.4 WP), front-end pages send `X-TEC-API-VERSION: unsupported` (`V1/Main.php:170-175`, `common/src/Tribe/REST/Headers/Unsupported.php:32-38`).

### 1.2 Category/tag route availability
`/categories*` and `/tags*` are registered only when `$wp_version >= 4.7` (`V1/Main.php:221-224`).

### 1.3 Other global behaviour
- `tribe_register_rest_route()` (`common/src/functions/utils.php:812-853`) runs the filters `tribe_register_rest_route_args_{namespace}{route}` and `tribe_register_rest_route_args`, so third-party code can change route args. If a single-handler route has a `callback` but no `permission_callback`, it gets `'__return_true'` (lines 844-850), which makes it public.
- The event category taxonomy is forced to `show_in_rest = true` (`V1/Main.php:923-927`).
- **[WP core]** Global query parameters work on every route: `_fields` (top-level response key filtering), `_envelope` (wraps the response as `{"body":…,"status":…,"headers":…}` with HTTP 200), `_jsonp`, `_method` (method override for POST), and `context`. `context=edit` is significant only for password checks (§3.6).

---------------------------------------------------------------------------------------------------

## 2. Route table

HTTP method sets **[WP core]**: `READABLE` = `GET` (HEAD is served as GET). `CREATABLE` = `POST`. `EDITABLE` = `POST, PUT, PATCH`. `DELETABLE` = `DELETE`.

| # | Path regex (relative to namespace) | Methods | Handler | Permission check | Source |
|---|---|---|---|---|---|
| 1 | `/doc` | GET | `Swagger_Documentation::get` | public (auto `__return_true`) | `V1/Main.php:241-244` |
| 2 | `/events` | GET | `Archive_Event::get` | public (auto) | `V1/Main.php:299-303` |
| 3 | `/events` | POST | `Single_Event::create` | `Single_Event::can_create` | `V1/Main.php:356-364` |
| 4 | `/events/(?P<id>\d+)` | GET | `Single_Event::get` | `__return_true` | `V1/Main.php:331-340` |
| 5 | `/events/(?P<id>\d+)` | DELETE | `Single_Event::delete` | `Single_Event::can_delete` | `V1/Main.php:341-346` |
| 6 | `/events/(?P<id>\d+)` | POST, PUT, PATCH | `Single_Event::update` | `Single_Event::can_edit` | `V1/Main.php:347-352` |
| 7 | `/events/by-slug/(?P<slug>[^/]+)` | GET | `Single_Event_Slug::get` | `__return_true` | `V1/Main.php:392-401` |
| 8 | `/events/by-slug/(?P<slug>[^/]+)` | DELETE | `Single_Event_Slug::delete` | `can_delete` (slug-resolved) | `V1/Main.php:402-407` |
| 9 | `/events/by-slug/(?P<slug>[^/]+)` | POST, PUT, PATCH | `Single_Event_Slug::update` | `can_edit` (slug-resolved) | `V1/Main.php:408-413` |
| 10 | `/venues` | GET | `Archive_Venue::get` | public (auto) | `V1/Main.php:737-741` |
| 11 | `/venues` | POST | `Single_Venue::create` | `Single_Venue::can_create` | `V1/Main.php:547-556` |
| 12 | `/venues/(?P<id>\d+)` | GET / DELETE / POST,PUT,PATCH | `Single_Venue::get/delete/update` | `__return_true` / `can_delete` / `can_edit` | `V1/Main.php:522-545` |
| 13 | `/venues/by-slug/(?P<slug>[^/]+)` | GET / DELETE / POST,PUT,PATCH | `Single_Venue_Slug::…` | same, slug-resolved | `V1/Main.php:582-605` |
| 14 | `/organizers` | GET | `Archive_Organizer::get` | public (auto) | `V1/Main.php:764-768` |
| 15 | `/organizers` | POST | `Single_Organizer::create` | `Single_Organizer::can_create` | `V1/Main.php:656-665` |
| 16 | `/organizers/(?P<id>\d+)` | GET / DELETE / POST,PUT,PATCH | `Single_Organizer::…` | `__return_true` / `can_delete` / `can_edit` | `V1/Main.php:631-654` |
| 17 | `/organizers/by-slug/(?P<slug>[^/]+)` | GET / DELETE / POST,PUT,PATCH | `Single_Organizer_Slug::…` | same, slug-resolved | `V1/Main.php:691-714` |
| 18 | `/categories` | GET | `Archive_Category::get` | `__return_true` | `V1/Main.php:794-803` |
| 19 | `/categories` | POST | `Single_Category::create` | `Single_Category::can_create` | `V1/Main.php:804-809` |
| 20 | `/categories/(?P<id>\d+)` | GET / POST,PUT,PATCH / DELETE | `Single_Category::get/update/delete` | `__return_true` / `can_edit` / `can_delete` | `V1/Main.php:813-836` |
| 21 | `/tags` | GET | `Archive_Tag::get` | `__return_true` | `V1/Main.php:864-873` |
| 22 | `/tags` | POST | `Single_Tag::create` | `Single_Tag::can_create` | `V1/Main.php:874-879` |
| 23 | `/tags/(?P<id>\d+)` | GET / POST,PUT,PATCH / DELETE | `Single_Tag::get/update/delete` | `__return_true` / `can_edit` / `can_delete` | `V1/Main.php:883-906` |

There are **no** by-slug routes for categories or tags. **[WP core]** also adds the namespace index `GET /wp-json/tribe/events/v1` (returns `{"namespace":"tribe/events/v1","routes":{…}}`).

### 2.1 Authentication and authorization
TEC adds **no authentication of its own**. `V1/Auth.php` (`can_post_event`) is dead code: no route uses it. Identity comes from standard WordPress REST auth **[WP core]**:
- cookie + `X-WP-Nonce: <wp_rest nonce>` header (or `_wpnonce` param). A cookie without a nonce is treated as logged out.
- Application Passwords: HTTP Basic `Authorization: Basic base64(user:app-password)` (WP ≥ 5.6, HTTPS required by default).
- Any auth plugin (JWT, OAuth).

Permission callbacks (capabilities resolved via `get_post_type_object(...)->cap`):

| Callback | Logic | Source |
|---|---|---|
| `Single_Event::can_create` | `current_user_can( tribe_events.cap.edit_posts )` | `V1/Endpoints/Single_Event.php:505-509` |
| `Single_Event::can_edit` | no id, or no post with that id → `edit_posts`. Otherwise `edit_post` meta-cap on that id. | `Single_Event.php:665-676` |
| `Single_Event::can_delete` | no id, or no post → `delete_posts`. Otherwise `delete_post` on that id. | `Single_Event.php:581-592` |
| `Single_Venue::can_create/edit/delete` | `tribe_venue` caps. `edit_post`/`delete_post` on the id when one is given (no existence fallback). | `Single_Venue.php:461-465, 532-540, 593-601` |
| `Single_Organizer::can_create/edit/delete` | the same with `tribe_organizer` caps | `Single_Organizer.php:327-331, 467-475, 528-536` |
| `Term_Single_Base::can_create` (also `can_edit`, `can_delete`) | `current_user_can( tribe_events.cap.edit_posts ) && current_user_can( taxonomy.cap.manage_terms )`. The term id is **not** checked per item. | `V1/Endpoints/Term_Single_Base.php:121-136, 197-199, 250-258` |
| Slug variants | `Slug_Endpoint` trait: the slug is resolved to `id` (`get_page_by_path`) **before** the permission check and the handler run. An unresolved slug gives `id = 0`. Any `id` in the body or query is overwritten. A scalar JSON body is discarded. | `V1/Endpoints/Slug_Endpoint.php:34-45, 97-120` |

**Failure responses [WP core]:**
- A permission callback returning `false` gives `{"code":"rest_forbidden","message":"Sorry, you are not allowed to do that.","data":{"status":401}}` when not logged in, and `status: 403` when logged in.
- **Order of checks [WP core]:** route match → **argument validation** (`validate_callback`, required) → sanitization → **permission_callback** → handler. So a bad or nonexistent id (for venue, organizer, category, tag and slug routes, where existence is part of the validator) gives **400 `rest_invalid_param`** even for anonymous callers, **before** any 401/403.

Read access inside public GET handlers:
- Events archive: anonymous users only see `publish`. Users with `edit_posts` default to `status=any`. In both cases the query is further limited to posts the user can read (`Archive_Base.php:202-340`; `Archive_Event.php:212-217, 250-269, 285-288`).
- Single event: `publish`, or `read_post` cap. Trash is always rejected. Password protection is enforced (§3.6) (`Single_Event.php:109-134`).
- Single venue/organizer: `publish` or `read_post` cap (`Single_Venue.php:30-35`; `Single_Organizer.php:26-32` checks the **venue** post type's `read_post` cap for organizers, which is a source bug with no practical effect).

---------------------------------------------------------------------------------------------------

## 3. Parameter handling (applies everywhere)

### 3.1 How WordPress processes args [WP core]
- Params merge from (highest priority first) JSON body (with `Content-Type: application/json`), form body (POST), query string, URL captures (`id`, `slug`), then registered `default`s.
- `$request['x']` / `isset($request['x'])` **include defaults**. A param with a `default` is therefore always "set".
- `validate_callback` runs on **every non-null param, including defaults**. A `false` return gives a 400 `rest_invalid_param`. A returned `WP_Error` also gives a 400 `rest_invalid_param`, with the inner error in `data.params`/`data.details`.
- If an arg has `type` but no `sanitize_callback`, WP applies `rest_parse_request_arg`. That validates against the JSON-schema `type` (e.g. `integer`, `boolean`, `string`) and coerces: `"5"` → `5`, `"true"|"1"` → `true`, `"false"|"0"` → `false`. Booleans accept `true/false/"true"/"false"/"1"/"0"/1/0`.
- Args **without** `type` (e.g. `categories`, `venue`, `include`, `has_events`) are passed through raw: string, or array if sent as `x[]=…`.

### 3.2 Empty-string pitfall
Every validator named `is_string` (`common/src/Tribe/Validator/Base.php:33-35`) is `! empty($v) && is_string($v)`. Sending `search=`, `password=`, `strict_dates=`, `order=`, `slug=` etc. with an **empty value gives 400**. The same applies to `categories=`, `tags=`, `include=`, `venue=`, `organizer=` (their list validators require ≥ 1 valid element) and `status=`. **Omit params rather than sending them empty.**

### 3.3 Lists: CSV vs arrays
`Tribe__Utils__Array::list_to_array( $value, ',' )` (`common/src/Tribe/Utils/Array.php:154-182`):
- Accepts an array (`x[]=1&x[]=2`, or a JSON array) or a string.
- A string is `urldecode`d, then split on `/\s*,\s*/`. Empty items are dropped. Numeric items are converted to numbers.
- Therefore CSV `x=1,2,3`, `x=1, 2`, and the array form are all equivalent. Mixed slug/ID lists are fine for `categories`/`tags`.

### 3.4 Date formats (`is_time`)
`common/src/Tribe/Validator/Base.php:57-59`:
```php
return is_numeric( $value ) || ( is_string( $value ) && strtotime( $value ) );
```
- Accepted: any `is_numeric` value (treated as a **Unix timestamp** downstream when `(int)$v == $v`, see `Date_Utils::is_timestamp` at `common/src/Tribe/Date_Utils.php:454-460`), or any string PHP `strtotime()` can parse. Examples: `2024-05-01`, `2024-05-01 18:00:00`, `2024-05-01T18:00:00Z`, `2024-05-01T18:00:00+02:00`, `now`, `tomorrow`, `+1 week`, `next monday`.
- **Gotcha:** a bare year like `2024` passes `is_numeric` and is interpreted as timestamp 2024 (1970-01-01T00:33:44Z).
- A string `strtotime()` parses to exactly `0` (`1970-01-01 00:00:00 UTC`) fails validation.
- **Recommended client format:** `YYYY-MM-DD HH:MM:SS` (site-local wall time) or ISO-8601 with an explicit offset.

### 3.5 Default `per_page`
`Base::get_default_posts_per_page()` (`V1/Endpoints/Base.php:201-205`) returns the TEC option `posts_per_page`, else the WP option `posts_per_page`, else `20`. The value is site-dependent; WP's own default is `10`. Archive `per_page` values are passed through `sanitize_per_page` (`V1/Endpoints/Archive_Base.php:92-96`): `min( max_per_page, intval($v) )`. **Values above the max are silently clamped (no error).** Defaults are clamped too.

| Archive | Max-per-page filter | Default max |
|---|---|---|
| events | `tribe_rest_event_max_per_page` | 50 (`Archive_Event.php:464-471`) |
| venues | `tribe_rest_venue_max_per_page` | 50 (`Archive_Venue.php:302-309`) |
| organizers | `tribe_rest_organizer_max_per_page` | 50 (`Archive_Organizer.php:308`) |
| categories **and** tags | `tribe_rest_event_category_max_per_page` | 50 (`Term_Archive_Base.php:117-124`) |

### 3.6 Password-protected posts
- `can_access_password_content()` (`V1/Validator/Base.php:195-222`) returns true when: the post has no password; OR `context=edit` and the user has `edit_post`; OR the request param `password` (query or body) `hash_equals` the post password.
- **Single event** (`/events/{id}`, `/events/by-slug/{slug}`): without access, the response is **403 `event-password-protected`** (`Single_Event.php:127-131`). `password` is a declared arg only on the by-id route (`Single_Event.php:294-300`). The slug route still reads `$request['password']`, but it is undeclared, so it is not validated there.
- **Events archive:** password-protected events are **not dropped**. Their protected fields are blanked (§4.10) unless `password` (one value for all events) or `context=edit` grants access (`V1/Main.php:133-159`).
- **Venue/organizer** data (single, archive and embedded): blanked by the `tribe_rest_venue_data`/`tribe_rest_organizer_data` filter at priority 100 (`V1/Main.php:65-66, 113-121`) when `post_password_required()`. The single and archive venue/organizer endpoints first apply WP core `WP_REST_Posts_Controller::can_access_password_content` (`password` param or `context=edit`). If that grants access, they temporarily bypass the check (`Single_Venue.php:37-50`, `Archive_Venue.php:186-204`).

---------------------------------------------------------------------------------------------------

## 4. Response object shapes

### 4.1 Event object
Built by `Post_Repository::get_event_data( $event_id, $context )` (`V1/Post_Repository.php:71-204`). `$context` is `'single'` for `GET /events/{id}` and by-slug (`Single_Event.php:162`), and `''` everywhere else (archive, create, update, delete).

Key order as emitted:

| Key | Type (wire) | Notes / source line (`Post_Repository.php`) |
|---|---|---|
| `id` | int | `$event_id` (125). An int in practice because WP coerces the `id` route arg (type integer). |
| `global_id` | str | e.g. `"example.com?id=123"`: host + path + `?id=`, **no scheme** (`add_global_id_fields` 414-425; `common/src/Tribe/Utils/Global_ID.php:126-133, 80-104`). Initialised to `false` (126) and always overwritten. It would be `false` only if Global_ID generation failed (practically never). |
| `global_id_lineage` | str[] | `[global_id, …Event-Aggregator lineage meta…]` (422) |
| `author` | **digits-string** | `$post->post_author` (128). WP_Post stores it as a string. Swagger claims integer. Accept string-or-int. |
| `status` | str | WP post status: `publish`, `draft`, `pending`, `private`, `future`, `trash`, … |
| `date` | DB datetime | `post_date`, site-local (130) |
| `date_utc` | DB datetime | `post_date_gmt` (131). Can be `"0000-00-00 00:00:00"` for unpublished drafts. |
| `modified` | DB datetime | (132) |
| `modified_utc` | DB datetime | (133) |
| `url` | str | permalink (134). In theory `get_the_permalink` can return `false`. |
| `rest_url` | str | `…/tribe/events/v1/events/{id}` (135) |
| `title` | str | after the `the_title` filter: **HTML entities** such as `&#8217;` and `&amp;` are possible (136) |
| `description` | str | `post_content` after `the_content`: **HTML** (137) |
| `excerpt` | str | after `the_excerpt`: HTML, usually `<p>…</p>` or `""` (138) |
| `slug` | str | `post_name`. Can be `""` for drafts. Trashed posts get the `__trashed` suffix. |
| `image` | **`false` \| Image** | §4.4 (140, 362-404) |
| `all_day` | bool | `tribe_is_truthy(_EventAllDay)`. Absent meta gives `false` (141). |
| `start_date` | DB datetime | `_EventStartDate`, **event-local** wall time (142). Missing meta (corrupt data) gives `null` plus a PHP warning. |
| `start_date_details` | DateDetails | §4.5 (143) |
| `end_date` | DB datetime | `_EventEndDate`, event-local (144) |
| `end_date_details` | DateDetails | (145) |
| `utc_start_date` | DB datetime | `_EventStartDateUTC` (146) |
| `utc_start_date_details` | DateDetails | (147) |
| `utc_end_date` | DB datetime | `_EventEndDateUTC` (148) |
| `utc_end_date_details` | DateDetails | (149) |
| `timezone` | str | `""` if no `_EventTimezone` meta. Otherwise the stored tz if valid, else the site tz (150; `src/Tribe/Timezones.php:74-83`). The site tz may be an IANA id **or** a `"UTC+0"` / `"UTC-5"` / `"UTC+5.5"` pseudo-id (`common/src/Tribe/Timezones.php:68-93`). Do not parse it with a strict IANA type. |
| `timezone_abbr` | str | `_EventTimezoneAbbr` or `""` (151), e.g. `"EDT"`, `"+0530"`, `""` |
| `cost` | str | §4.6 (152) |
| `cost_details` | CostDetails | §4.6 (153-158) |
| `website` | str | `esc_html(_EventURL)` when the meta key exists (may be `""`, and `&` becomes `&amp;`). **If the meta key is absent, falls back to the event permalink** (159). |
| `show_map` | bool | `(bool)_EventShowMap`. Absent meta gives `true` (160). |
| `show_map_link` | bool | same (161) |
| `hide_from_listings` | bool | `_EventHideFromUpcoming === 'yes'` (162) |
| `sticky` | bool | `menu_order == -1` (163) |
| `featured` | bool | `_tribe_featured == 1` (164) |
| `categories` | Term[] (embedded variant, §4.7.1) | (165, 615-627). `array_filter` preserves keys: if a filter removes a middle element this could serialise as a JSON object. Not observed in core. |
| `tags` | Term[] (embedded variant) | (166, 686-698) |
| `venue` | **`[]` \| Venue \| Venue[]** | (167). See below. |
| `organizer` | **Organizer[]** (possibly `[]`) | (168). See below. |
| `custom_fields` | **`[]` \| `{ "<meta_key>": {"label": str, "value": str} }`** | (108-122, 169). Only for TEC/ECP "additional fields" with a non-empty value. The key is the meta key, e.g. `_ecp_custom_2`. **Empty encodes as JSON `[]`, non-empty as a JSON object.** Checkbox values are pipe-joined strings. |
| `json_ld` | object | **Only when context is `'single'`** (filter `tribe_rest_event_json_ld_data_contexts`, default `['single']`) **and** JSON-LD generation returned data (180-188). Absent for anonymous users on non-public or password-protected events (`common/src/Tribe/JSON_LD/Abstract.php:61-93`). §4.9. |

**`venue` polymorphism** (`get_venue_data`, 216-360):
- Event has no venue → `WP_Error` internally → `[]` (167).
- Exactly one readable venue → **a single Venue object** (`count($data)===1 ? reset($data)`, 359).
- Several venues (possible via the `tec_get_venue_ids` filter or add-ons) → **JSON array of Venue**.
- Venues the current user cannot read (non-`publish` without `read_post`) are dropped (`V1/Main.php:67-68, 84-102`). If all are dropped, the result is `[]`.

Rust: `#[serde(untagged)] enum VenueField { One(Venue), Many(Vec<Venue>) }`. An empty `[]` lands in `Many(vec![])`.

**`organizer`** (`get_organizer_data` with an event id, 439-555): always a **JSON array** (`$single=false`). It is `[]` when there are none (the `WP_Error` from 443 maps to `[]` at 168).

Fields added by **other plugins** through the `tribe_rest_event_data` filter (199). They are not produced by TEC core, but TEC's password-blanking list names them (`V1/Validator/Base.php:16-59`). Expect them when those plugins are active:

| Key | Type | Plugin |
|---|---|---|
| `ticketed` | bool (or `null` when blanked) | Event Tickets |
| `attendance` | `{ "total_attendees": int\|null, "checked_in": int\|null, "not_checked_in": int\|null }` | Event Tickets Plus |
| `is_virtual` | bool \| null | Virtual Events (now part of ECP) |
| `virtual_url` | str | Virtual Events |
| `virtual_video_source` | str | Virtual Events |
| (others) | unknown | ECP (recurrence), Filter Bar, Community, … |

The `tribe_rest_single_event_data` filter (`Single_Event.php:172`) can modify only the single-event response. **The client must ignore unknown fields** (no `deny_unknown_fields`).

### 4.2 Venue object
`get_venue_data` (`Post_Repository.php:274-345`). **Important:** the base array goes through `array_filter()` (325). **Every falsy value is removed**: `""`, `"0"`, `0`, `false`, `[]`. So all string fields below are **optional (absent when empty)**.

| Key | Type | Presence |
|---|---|---|
| `id` | int | always (non-zero) |
| `author` | digits-string | absent if `"0"` |
| `status` | str | always |
| `date`, `date_utc`, `modified`, `modified_utc` | DB datetime | normally present (`"0000-00-00 00:00:00"` is truthy and kept) |
| `url` | str | permalink |
| `venue` | str | the venue **name**, after `the_title` (HTML entities possible). Absent if the title is empty. |
| `description` | str (HTML) | optional |
| `excerpt` | str (HTML) | optional |
| `slug` | str | optional |
| `image` | Image | optional. **Absent** (not `false`) when there is no image, because `false` is filtered. |
| `address`, `city`, `country`, `province`, `state`, `zip`, `phone`, `website`, `stateprovince` | str | optional each (meta `_VenueAddress`, `_VenueCity`, …, `_VenueURL`, `_VenueStateProvince`) |
| `geo_lat`, `geo_lng` | number (float) | **only with Events Calendar Pro** and both non-zero (`src/functions/template-tags/venue.php:660-683`; `Post_Repository.php:300-305`). TEC alone yields lat/lng `0`, so these keys are absent. |
| `json_ld` | object | only when context is `'single'` (filter `tribe_rest_venue_json_ld_data_contexts`). That covers `GET /venues/{id}`, by-slug, **and venues embedded in `GET /events/{id}`** (the single-event context propagates, 105). |
| `show_map` | bool | **always** (added after the filter, 327). `true` when the meta is absent. With a password, can be `null` (§4.10). |
| `show_map_link` | bool | always (328), same caveat |
| `global_id` | str | always (331) |
| `global_id_lineage` | str[] | always |

### 4.3 Organizer object
`get_organizer_data` (`Post_Repository.php:489-539`). `array_filter()` is applied to the whole array, including `global_id`, before the `tribe_rest_organizer_data` filter (535). **All falsy fields are absent.**

| Key | Type | Presence |
|---|---|---|
| `id` | int | always |
| `author` | digits-string | optional |
| `status` | str | |
| `date`, `date_utc`, `modified`, `modified_utc` | DB datetime | |
| `url` | str | |
| `organizer` | str | the organizer **name** (HTML entities possible), optional |
| `description`, `excerpt` | str (HTML) | optional |
| `slug` | str | optional |
| `image` | Image | optional (absent when there is none) |
| `phone`, `website`, `email` | str | optional (`_OrganizerPhone`, `_OrganizerWebsite`, `_OrganizerEmail`) |
| `json_ld` | object | only in `'single'` context (filter `tribe_rest_organizer_json_ld_data_contexts`). Includes organizers embedded in `GET /events/{id}`. |
| `global_id` | str | present |
| `global_id_lineage` | str[] | present |

Organizers have no `show_map`.

`GET /organizers/{id}` returns `get_organizer_data($id,'single')` with `$single=true`. That is `$data[0] ?? []`, so it is an object, or `[]` if it was filtered away (554).

### 4.4 Image object (`Post_Repository::get_featured_image`, 362-404)
- No thumbnail → **`false`** for events. For venues/organizers the key is absent.
- Otherwise the base is `{ "url": str|false, "id": int, "extension": str }`. `url` comes from `get_the_post_thumbnail_url(...,'full')`. `extension` is the file extension of the attached file.
- If attachment metadata has `image_meta`, `file` **and** `sizes`, those keys (minus `image_meta` and `file`) are merged in, then **`array_filter`** removes falsy values. Typical result:
```json
{
  "url": "https://…/photo.jpg",
  "id": 42,
  "extension": "jpg",
  "width": 1920,
  "height": 1080,
  "filesize": 345678,            // WP ≥ 6.0 only; may be absent
  "original_image": "photo.jpg", // only when WP scaled a big upload; may be absent
  "sizes": {                     // absent if no sub-sizes ([] is falsy)
    "thumbnail": { "width": 150, "height": 150, "mime-type": "image/jpeg", "filesize": 5000, "url": "https://…-150x150.jpg" },
    "medium":    { … },
    "tribe_event_large": { … }
  }
}
```
- `sizes` is a **JSON object keyed by size name**, not an array (the Swagger "array" is wrong). Each size: `width` int, `height` int, `mime-type` str, `filesize` int (optional, WP ≥ 6.0), `url` str (`""` if it could not be resolved). The `file` key is removed (388-392).
- When the metadata condition fails (e.g. SVG or a missing `image_meta`), only `{url,id,extension}` is returned and `array_filter` is **not** applied, so `extension` may be `""`.
- Other keys WP stores in attachment metadata can appear (e.g. `sources` from some plugins).
- The whole object passes through the `tribe_rest_event_featured_image` filter (403). Since that filter name is used for venues and organizers too, the shape may vary.

### 4.5 DateDetails (`get_date_details`, 562-573)
```json
{ "year": "2024", "month": "05", "day": "01", "hour": "18", "minutes": "00", "seconds": "00" }
```
**All values are zero-padded strings** (PHP `date('Y'|'m'|'d'|'H'|'i'|'s')`). Swagger claims integers. The input is the DB datetime parsed with `strtotime` in PHP's default timezone (UTC in WP), so the details mirror the string components exactly. A null or invalid input yields the 1970 epoch values.

### 4.6 Cost
- `cost` (str): `tribe_get_cost($id, true)` → `Cost_Utils::get_formatted_event_cost` (`src/Tribe/Cost_Utils.php:94-143`), then the `tribe_get_cost` filter.
  - `""` when there is no cost.
  - A single value or a range `"min – max"`. The separator is `" – "`: spaces around U+2013, translatable.
  - Values can be replaced by the localized `"Free"` (`maybe_replace_cost_with_free`).
  - The currency symbol is placed per `_EventCurrencyPosition` or the `reverseCurrencyPosition` option.
  - Each part is `esc_html`'d, so HTML entities are possible (e.g. `&#36;` or `&amp;`).
  - Event Tickets (via the filter) can replace it with ticket price ranges.
- `cost_details`:
```json
{ "currency_symbol": str, "currency_code": str, "currency_position": str, "values": [str, …] }
```
  - `currency_symbol` = `_EventCurrencySymbol` or `""`.
  - `currency_code` = `_EventCurrencyCode` or `""` (ISO 4217 when set).
  - `currency_position` = `_EventCurrencyPosition` or `""`. **Observed values: `""`, `"prefix"`, `"suffix"` (admin UI), `"postfix"` (REST-created events, `common/src/Tribe/Cost_Utils.php:486`).** Swagger says `prefix|postfix`. Model as a string or an enum with an `Other` fallback.
  - `values`: `get_cost_values` (`Post_Repository.php:582-604`) returns the parsed costs sorted numerically. Numeric costs come out as **numeric strings** (decimal separator normalised to `.`, thousands separator removed), e.g. `["10","25.5"]`. Non-numeric costs (e.g. "Free", "Donation") are emitted as their **index key** (`sanitize_title` slug, e.g. `"free"`). An int-like key can become a JSON **number**, so `values` can mix strings and numbers. Swagger says `integer[]`. Use `Vec<StringOrNumber>`.

### 4.7 Term objects — two different shapes

#### 4.7.1 Embedded in an Event (`categories`/`tags` inside an event)
Built from a cast `WP_Term` (`Post_Repository::get_terms` 639-675 → `prepare_terms_data` 710-728 → `prepare_term_data` 741-789):
```json
{
  "id": 12,                    // renamed from term_id (int)
  "name": "Concerts",          // may contain HTML entities (&amp;)
  "slug": "concerts",
  "term_group": 0,             // int
  "term_taxonomy_id": 12,      // int
  "taxonomy": "tribe_events_cat" | "post_tag",
  "description": "",
  "parent": 0,                 // int (0 for tags / top-level)
  "count": 3,                  // int
  "filter": "raw",
  "urls": {
    "self": ".../tribe/events/v1/categories/12",   // "tags/…" for tags
    "collection": ".../tribe/events/v1/categories",
    "up": ".../tribe/events/v1/categories/5"       // ONLY when parent != 0
  }
}
```
There is **no** `url`/`link` key in this variant.

#### 4.7.2 From `/categories`, `/tags`, `/categories/{id}`, `/tags/{id}`
Built from **[WP core]** `WP_REST_Terms_Controller::prepare_item_for_response` output, then `prepare_term_data` renames `link`→`url`, strips `_links` and adds `urls`:
```json
{
  "id": 12, "count": 3, "description": "", "url": "https://site/events/category/concerts/",
  "name": "Concerts", "slug": "concerts", "taxonomy": "tribe_events_cat",
  "parent": 0,                // categories only (hierarchical); ABSENT for tags
  "meta": [],                 // [] or {…} if term meta is registered for REST
  "urls": { "self": "…", "collection": "…", "up": "…" /* only if parent */ }
}
```
The `tribe_rest_taxonomy_term_data` filter (786) applies to both variants. `tribe_rest_event_taxonomy_term_data` (671) applies only to the embedded variant. **[WP core]** `_fields` and `context` also affect the 4.7.2 fields (`context=edit` is not useful here).

Rust: use one struct with optional `term_group`, `term_taxonomy_id`, `filter`, `url`, `meta`, `parent`.

### 4.8 Archive wrappers

**Events** (`Archive_Event.php:233-298`), key order:
```json
{
  "events": [Event, …],             // always present; [] if none
  "rest_url": "…/events/?page=1&per_page=…&start_date=…&end_date=…&status=publish…",
  "next_rest_url": "…",             // ONLY if another page exists
  "previous_rest_url": "…",         // ONLY if page > 1 and the previous page is non-empty
  "total": 123,                     // int
  "total_pages": 3                  // int (0 when total is 0)
}
```
**Venues / Organizers** (`Archive_Venue.php:184-222`): `{ "venues"|"organizers": [...], "rest_url", "next_rest_url"?, "previous_rest_url"?, "total": int, "total_pages": int }`.

**Categories / Tags** (`Term_Archive_Base.php:85-102`): `{ "rest_url", "total", "total_pages", "categories"|"tags": [...], "next_rest_url"?, "previous_rest_url"? }`. `total` and `total_pages` come from **[WP core]** `X-WP-Total` and `X-WP-TotalPages`, which are ints.

Notes on archive URLs (all quirky, so treat them as hints):
- `rest_url` is rebuilt from internal query args (`Archive_Base::get_current_rest_url`, 137-165) through `supported_query_vars` reversal.
  - Events (`Archive_Event.php:235`): includes `page`, `per_page`, `start_date`, `end_date` (already **post-processed** values, e.g. `2024-05-01 00:00:00`), `search`, `status` (`publish`/`any`/list), `include` (CSV), `post_parent`, `starts_*`/`ends_*`, `ticketed`, `geoloc*`, plus `venue`, `organizer`, `featured` from `extra_rest_args`.
  - **`categories` and `tags` filters are NOT carried into `rest_url`/`next_rest_url`** (they live in `tax_query`, which is not in the map). Following `next_rest_url` therefore **drops category/tag filtering**.
  - **`featured=false` is also dropped** (`add_query_arg` removes `false` values), and `strict_dates` is never included.
  - **Recommendation:** build page URLs yourself (same params, increment `page`) rather than following `next_rest_url`.
- **[WP core]** `add_query_arg` does **not** urlencode newly added values. `rest_url` can contain raw spaces and colons (e.g. `start_date=2024-05-01 00:00:00`). `next_rest_url`/`previous_rest_url` are rebuilt via `remove_query_arg`, which re-encodes existing params, so they are percent/plus-encoded. Parse URLs leniently.
- `previous_rest_url` for page 2 has **no** `page` param (`Archive_Base.php:187-191`).
- The venue/organizer `rest_url` drops `search` (it is converted to `post__in`, which is not in their var map). It includes `page`, `per_page`, `event`, `has_events`, `status`.
- The term archive `rest_url` is the **bare base** (`…/categories/`) with **no** filters (`Term_Archive_Base.php:86`). `next_rest_url` is `base?page=N`, which loses `per_page` and every filter.

### 4.9 `json_ld`
Opaque schema.org object (`src/Tribe/JSON_LD/Event.php:85-156`, `common/src/Tribe/JSON_LD/Abstract.php:61-130`).
- Event fields: `@context` `"http://schema.org"`, `@type` `"Event"`, `name`, `description`, `image`?, `url`, `eventAttendanceMode`/`eventStatus` (via filters), `startDate`/`endDate` (ISO-8601 with offset), `location` (a Place object, if there is a venue), `organizer` (a Person/Organization object, if there is an organizer), `offers` (`{ "@type":"Offer", "price": str, "priceCurrency": str, "url", "category":"primary", "availability":"inStock", "validFrom": ISO-8601 }`, if there is a cost), `performer: "Organization"`.
- `name`/`description` pass through `esc_js` (`\'` escapes and HTML entities).
- Recommend `serde_json::Value`.

### 4.10 Password-protected blanking (`remove_password_protected_content`, `V1/Validator/Base.php:231-253`)
For each **existing** key matching the list (and recursively inside nested arrays and objects), the value is replaced:

| Key | Replacement |
|---|---|
| `description`, `excerpt`, `cost`, `website`, `virtual_url`, `virtual_video_source`, `email`, `phone`, `stateprovince`, `zip`, `province`, `country`, `city`, `address` | `""` |
| `image` | `false` |
| `all_day`, `show_map`, `show_map_link`, `hide_from_listings`, `sticky`, `featured`, `ticketed`, `is_virtual` | **`null`** |
| `cost_details` | `{"currency_symbol":"","currency_position":"","currency_code":"","values":[]}` |
| `categories`, `tags`, `venue`, `organizer`, `custom_fields` | `[]` |
| `attendance` | `{"total_attendees":null,"checked_in":null,"not_checked_in":null}` |
| `geo_lat`, `geo_lng` | `0` (int) |

Filterable via `tec_events_rest_api_password_protected_fields`. `title`, dates, ids and urls are kept.

**Consequence for typing:** in the events archive, the boolean fields of `Event` must be `Option<bool>`. The same holds for venue `show_map`/`show_map_link`.

---------------------------------------------------------------------------------------------------

## 5. Response headers

### 5.1 On REST responses
| Header | Value | Where |
|---|---|---|
| `X-TEC-Total` | integer (as a header string): total matching items | events archive (`Archive_Event.php:313-316`), venues (`Archive_Venue.php:229-232`), organizers (same), categories/tags (`Term_Archive_Base.php:104-105`). Equals body `total`. |
| `X-TEC-TotalPages` | integer: total pages for the effective `per_page` | same routes. Equals body `total_pages`. |
| **[WP core]** `Link: <…/wp-json/>; rel="https://api.w.org/"`, `Allow: GET, POST, …`, `X-Robots-Tag: noindex`, `X-Content-Type-Options: nosniff`, CORS headers | — | all |
| **[WP core]** `Location` | the new term URL | term create (201) |

TEC does **not** set `X-WP-Total`/`X-WP-TotalPages` on its archives. The term archive builds a fresh response, so the core values are not forwarded as headers; they are only copied into the body. **[WP core]** CORS `Access-Control-Expose-Headers` does not list `X-TEC-*`. That only matters for browsers.

### 5.2 On front-end HTML pages (API discovery; not REST responses)
Sent at `template_redirect` (priority 11) and mirrored as `<meta>`/`<link>` in `wp_head` (`V1/Main.php:164-185`; `common/src/Tribe/REST/Headers/{Supported,Disabled,Unsupported}.php`; names in `V1/Headers/Base.php:9-25`):

| Header | Value |
|---|---|
| `X-TEC-API-VERSION` | `v1` (enabled), `disabled` (option or filter off), `unsupported` (no WP REST) |
| `X-TEC-API-ROOT` | context-dependent API URL (only when enabled). Single event page: `…/events/{id}`. Recurring-series "/all" page: `…/events?include=parent,child1,…`. Featured: `…/events/?featured=1`. Category archive: `…/events/?categories=<slug>`. Tag archive: `…/events/?tags=<slug>`. Venue page: `…/events/?venue=<id or slug>`. Otherwise the API root `…/tribe/events/v1/` (`V1/Headers/Base.php:82-164`). Not sent if the root cannot be computed. |
| `X-TEC-API-ORIGIN` | `home_url()` (only when enabled) |

HTML: `<meta name="tec-api-version" content="v1">`, `<meta name="tec-api-origin" content="{home_url}">`, `<link rel="alternate" href="{api_root}" />` (`common/src/Tribe/REST/Headers/Supported.php:19-29`).

---------------------------------------------------------------------------------------------------

## 6. Errors

### 6.1 Wire shape [WP core]
```json
{ "code": "string", "message": "string", "data": { "status": 404, "...": "…" } | null,
  "additional_errors": [ { "code": …, "message": …, "data": … } ]   // only when a WP_Error holds several codes
}
```
- The HTTP status is `data.status`. If a `WP_Error` has no `status` in its data, the HTTP status is **500**, and `data` may be `null` or a non-object.
- Validation errors (from the WP core arg pipeline):
  - `rest_missing_callback_param`, 400: `"Missing parameter(s): title, start_date"`, `data: {"status":400,"params":["title","start_date"]}`.
  - `rest_invalid_param`, 400: `"Invalid parameter(s): page"`, `data: {"status":400,"params":{"page":"Invalid parameter."},"details":{…}}`. When a validator returns a `WP_Error`, `params.<name>` holds its message and `details.<name>` holds `{code,message,data}`. Schema-type failures give messages like `"page is not of type integer."` with details code `rest_invalid_type`.
  - `rest_invalid_json`, 400: malformed JSON body.
- Others: `rest_no_route` (404, also for wrong-method requests), `rest_forbidden` (401/403).

Rust: `struct ApiError { code: String, message: String, data: Option<serde_json::Value>, additional_errors: Option<Vec<…>> }`. Then extract `status` and `params` leniently.

### 6.2 TEC message catalogue (`V1/Messages.php:17-131`)
`get_message()` returns the text below (translatable). Codes marked **used** are returned by some route, with the HTTP status the code sets. Unmarked ones are defined but unused by this API version (legacy).

| Code | Message (en) | HTTP | Used by (file:line) |
|---|---|---|---|
| `missing-event-id` | The event ID is missing from the request | — | unused |
| `event-not-found` | The requested post ID does not exist or is not an event | **500** (no status set) | `Post_Repository.php:97-99`. Reachable when create/update/delete fetch data for a post that vanished, e.g. `wp_trash_post` hard-deletes when `EMPTY_TRASH_DAYS=0`. |
| `event-no-venue` | The event does not have a venue assigned | — | internal only, mapped to `[]` |
| `event-no-organizer` | The event does not have an organizer assigned | — | internal only, mapped to `[]` |
| `event-not-accessible` | The requested event is not accessible | 403 | `Single_Event.php:121-125` |
| `event-password-protected` | The requested event is not accessible without a password | 403 | `Single_Event.php:127-131` |
| `event-archive-bad-page` | The 'page' parameter must be a positive integer greater than 1 | — | unused (WP validation is used instead) |
| `event-archive-bad-per-page` | The 'per_page' parameter must be a positive integer greater than 1 | — | unused |
| `event-archive-bad-start-date` | The 'start_date' parameter must be in a supported format | — | unused |
| `event-archive-bad-end-date` | The 'end_date' parameter must be in a supported format | — | unused |
| `event-archive-bad-search-string` | The 'search' parameter must be a string | — | unused |
| `event-archive-bad-categories` | The 'categories' parameter contains invalid category slugs or IDs | — | unused |
| `event-archive-bad-tags` | The 'tags' parameter contains invalid tag slugs or IDs | — | unused |
| `event-archive-page-not-found` | The requested event archive page does not exist | **404** | `Archive_Event.php:271-275` |
| `venue-not-found` | The requested post ID does not exist or is not an venue | — | internal (`Post_Repository.php:231`) |
| `organizer-not-found` | The requested post ID does not exist or is not an organizer | — | internal (`Post_Repository.php:454`) |
| `could-not-create-venue` | The venue could not be created from the provided data | 400 | `Single_Venue.php:80-84` (also nested in event create) |
| `could-not-create-organizer` | The organizer could not be created from the provided data | 400 | `Single_Organizer.php:77-81` (also nested) |
| `invalid-linked-post-id` | The provided linked post ID is not valid | 400 | `Linked_Post_Base.php:51-58` (event create/update with `venue: {"id": bad}`) |
| `venue-not-accessible` | The requested venue is not accessible | 403 | `Single_Venue.php:31-35` |
| `organizer-not-accessible` | The requested organizer is not accessible | 403 | `Single_Organizer.php:28-32` |
| `could-not-delete-event` | The event could not be deleted | 500 | `Single_Event.php:559-563` |
| `event-is-in-trash` | The event is already trashed | **410** | `Single_Event.php:538-542` |
| `could-not-delete-venue` | The venue could not be deleted | 500 | `Single_Venue.php:501-505` |
| `venue-is-in-trash` | The venue is already trashed | 410 | `Single_Venue.php:480-484` |
| `could-not-delete-organizer` | The organizer could not be deleted | 500 | `Single_Organizer.php:436-440` |
| `organizer-is-in-trash` | The organizer is already trashed | 410 | `Single_Organizer.php:415-419` |
| `could-not-update-event` | The event could not be updated | **403** | `Single_Event.php:626-630` |
| `could-not-update-venue` | The venue could not be updated | 400 | `Single_Venue.php:555-559` |
| `could-not-update-organizer` | The organizer could not be updated | 400 | `Single_Organizer.php:490-494` |
| `venue-archive-page-not-found` | The requested venue archive page does not exist | 404 | `Archive_Venue.php:178-182` |
| `organizer-archive-page-not-found` | The requested organizer archive page does not exist | 404 | `Archive_Organizer.php:178-182` |
| `archive-bad-search-string` | The 'search' parameter must be a string | 400 | `Archive_Venue.php:126-131`, `Archive_Organizer.php` (practically unreachable: `is_string` validation fires first) |
| `term-archive-page-not-found` | The requested event term archive page does not exist | — | unused (term archives never 404) |
| `rest-event-not-found` | `Event with ID %d does not exist.` (formatted with the id) | **404** | `Single_Event.php:77-84` (GET, DELETE, update by id; slug routes cannot reach it because validation fails first) |
| `rest-invalid-event-id` | The specified ID is not a valid event. | 400 | `Single_Event.php:87-94` (id is a post of another type) |
| `event-no-longer-available` | This event is no longer available | **403** | `Single_Event.php:111-118` (GET of a trashed event) |

Additional TEC error codes that are **not** in Messages.php:

| Code | Message | HTTP | Source |
|---|---|---|---|
| `rest-invalid-event-id` | `The "%s" parameter must be a valid numeric event ID.` / `The specified ID for the "%s" parameter is not a valid event.` | 400 | `Archive_Base.php:56-83`, used for events `post_parent`, venues/organizers `event`, terms `post`/`event`. In practice the arg `validate_callback` (`is_event_id`) fires first and you get `rest_invalid_param`. |
| `event-tickets-not-active` | Event Tickets plugin is not activated. | 400, but wrapped as `rest_invalid_param` → `data.details.ticketed.code` | `src/Tribe/Validator/Base.php:266-281` |
| `event-tickets-api-not-active` | Event Tickets REST API is not available. | 400, wrapped the same way | same |

Errors passed through from WordPress core or other layers:
- **[WP core]** `WP_REST_Terms_Controller`: `term_exists` (400, `data.term_id`), `rest_term_invalid` (404/400), `rest_taxonomy_not_hierarchical` (400, tags with `parent`), `rest_trash_not_supported` (501, term delete when the `tribe_events_rest_term_allow_delete` filter returns false), `rest_cannot_delete` (500).
- `wp_insert_post` errors on event create (e.g. `empty_content`, `invalid_date`, `db_insert_error`) normally have **no status, so HTTP 500**.
- `Tribe__Events__API` meta errors (`invalid-tribe-events-*-meta-value`, `src/Tribe/API.php:862-955`) are not reachable through REST, which sends `EventStartTime` directly.

---------------------------------------------------------------------------------------------------

## 7. Endpoint details

### 7.1 `GET /doc`
- No args (`Swagger_Documentation.php:174-176`). Returns an **OpenAPI 3.0.0** document (not Swagger 2.0, despite comments): `{openapi:"3.0.0", info:{version:"1.0.0", title, description}, components:{schemas:{ImageSize, CostDetails, Term, Image, DateDetails, Event, Venue, Organizer}}, servers:[{url}], paths:{…}}` (`Swagger_Documentation.php:62-87`, `V1/Main.php:249-257`).
- Path keys: `/doc`, `/events`, `/events/{id}`, `/events/by-slug/{slug}`, `/venues`, `/venues/{id}`, `/venues/by-slug/{slug}`, `/organizers`, `/organizers/{id}`, `/organizers/by-slug/{slug}`, `/categories`, `/categories/{id}`, `/tags`, `/tags/{id}`.
- `servers[0].url` is built as `{http|https}://{home host}{rest path}`, so the port is lost.
- The doc schemas are **inaccurate**: `author` is declared integer (actually a string); `*_date_details` are declared arrays of objects with integer fields (actually one object with string fields); `cost_details` is declared an array (actually an object); `cost_details.values` is declared `integer[]` (actually strings); the cost-details property name contains a trailing space (`'currency_position '`); `image.sizes` is declared an array (actually a keyed object); venue/organizer `image` is declared type string. Do not codegen from it.
- Filter: `tribe_rest_swagger_documentation`.

### 7.2 `GET /events` (Archive_Event)
Args (`Archive_Event.php:521-672`):

| Name | WP type | Required | Default | Validation / sanitize | Semantics |
|---|---|---|---|---|---|
| `page` | integer | no | `1` | `is_positive_int` (≥ 1, integral) | page number |
| `per_page` | integer | no | site default (§3.5) | `is_positive_int`; sanitized `min(50, intval)` | page size |
| `start_date` | (none; `swagger_type` string) | no | `localize_date('Y-m-d H:i:s','yesterday 23:59')` evaluated per request | `is_time` | lower bound on the event **start** (see 7.2.1) |
| `end_date` | (none) | no | `date('Y-m-d H:i:s', strtotime('+24 months'))` (UTC-based, no localisation) | `is_time` | upper bound on the event **end** |
| `starts_before` | (none) | no | — | `is_time` | event start < X |
| `starts_after` | (none) | no | — | `is_time` | event start > X |
| `ends_before` | (none) | no | — | `is_time` | event end < X |
| `ends_after` | (none) | no | — | `is_time` | event end > X |
| `strict_dates` | (none) | no | `"false"` | `is_string` (non-empty) | `filter_var(…, FILTER_VALIDATE_BOOLEAN)`: `"true"`, `"1"`, `"on"`, `"yes"` mean strict |
| `search` | string | no | — | `is_string` | WP `s` (title/content search) |
| `categories` | (none: CSV or array) | no | — | `is_event_category`: every item must `term_exists` in `tribe_events_cat`, by id or slug | OR across items (tax_query `IN`) |
| `tags` | (none: CSV or array) | no | — | `is_post_tag` | OR across items |
| `venue` | (none: CSV or array) | no | — | `is_venue_id_list`: every item numeric and `tribe_is_venue` | meta `_EventVenueID` = any |
| `organizer` | (none: CSV or array) | no | — | `is_organizer_id_list` | meta `_EventOrganizerID` = any |
| `featured` | boolean | no | — | WP boolean | `true` → only featured. `false` → only **non**-featured. Absent → both. |
| `status` | (none; `swagger_type` string) | no | — (`null`) | `filter_post_status_list`: CSV. Anonymous: exactly `publish`, else 400. Users with `edit_posts`: any registered stati. | if absent: `any` for editors, `publish` otherwise (`Archive_Event.php:212-217`) |
| `geoloc` | boolean | no | — | — | **ECP only** |
| `geoloc_lat` | (none; number) | no | — | — | ECP only, needs `geoloc` |
| `geoloc_lng` | (none; number) | no | — | — | ECP only |
| `include` | (none: CSV or array) | no | — | `is_positive_int_list`; sanitized to an array | `post__in`; orderby `[date, ID]` ASC (see 7.2.2) |
| `post_parent` | integer | no | — | `is_event_id`, then handler `validate_event_id_param` | `post_parent` (ECP recurrence children) |
| `ticketed` | boolean | no | — | `supports_ticketed`: `true` requires Event Tickets plus its REST API | ET filter |
| `password` | — | — | — | not declared but read (§3.6) | unlock password-protected events |
| `context` | — | — | — | not declared | `edit` unlocks password content for editors |

There are **no** `orderby`/`order` params for events.

#### 7.2.1 Date semantics (`Archive_Event.php:72-210`, ORM `src/Tribe/Repositories/Event.php`)
Mapping into the TEC ORM (`tribe_get_events` → `Tribe__Events__Query::getEvents` → `tribe_events()->by_args()`):

| Request param | ORM filter | SQL meaning | Source |
|---|---|---|---|
| `start_date` | `filter_by_starts_on_or_after` | start **>=** X | `Repositories/Event.php:184, 394-411` |
| `end_date` | `filter_by_ends_on_or_before` | end **<=** X | `Repositories/Event.php:185, 298-315` |
| `starts_after` | | start **>** X | `:362-379` |
| `starts_before` | | start **<** X | `:266-283` |
| `ends_after` | | end **>** X | `:426-443` |
| `ends_before` | | end **<** X | `:330-347` |

- **Comparison column:** if the TEC timezone mode is `site` ("use site timezone everywhere"), the comparison uses `_EventStartDateUTC`/`_EventEndDateUTC` and X is converted to UTC. Otherwise it uses the local `_EventStartDate`/`_EventEndDate` and X is converted to the **site** timezone (`Repositories/Event.php:94-102`).
- **Consequence:** the default window `start_date..end_date` returns only events that **start on or after the beginning of yesterday and end by the end of the day 24 months from now**. An event that started earlier but is still running is **excluded**. Multi-day events ending after the window are excluded.

**Inclusive (default) vs strict mode:**
- Default (`strict_dates` absent or false; the `tribe_events_rest_use_inclusive_start_end_dates` filter can override, `Archive_Event.php:120-165`):
  - `start_date` → `tribe_beginning_of_day($request['start_date'])`, i.e. date part + `00:00:00` (plus the TEC `multiDayCutoff` option offset).
  - `end_date` → `tribe_end_of_day(...)`, i.e. date part + `23:59:59` (plus cutoff).
  - **`starts_before`, `starts_after`, `ends_before`, `ends_after` are ALL replaced by `tribe_end_of_day(date)`** (`Archive_Event.php:150-164`), so the time component you send is discarded. Examples: `starts_after=2024-05-01T10:00` means start > `2024-05-01 23:59:59`. `ends_after=2024-05-01` means end > `2024-05-01 23:59:59`.
  - These helpers parse with `strtotime` in PHP's default tz (UTC) and keep only the date part. An explicit offset in the input can shift the date (`common/src/functions/template-tags/date.php:100-160`).
- Strict (`strict_dates=true`): each value goes through `Tribe__Timezones::localize_date('Y-m-d H:i:s', X)` (`common/src/Tribe/Timezones.php:337-359`). That is `new DateTime(X)` in PHP's default tz (**UTC** under WordPress), converted to the **site tz**. So **naive strings are interpreted as UTC** and shifted. Strings with an explicit offset or `Z` are honoured. Numeric values are Unix timestamps.

**Default window removal (`relative_dates`, `Archive_Event.php:201-210`):** if **any** of `starts_before`, `starts_after`, `ends_before`, `ends_after` is sent:
- `start_date` is **dropped** unless it is explicitly in the **query string**.
- `end_date` is dropped likewise.
- Note: it checks `get_query_params()`. A `start_date` sent in a POST/JSON body would not count, but this is a GET route anyway.

To fetch past events, send for example `starts_before=now` (removes the default window) or an explicit early `start_date`.

#### 7.2.2 `include` interaction (`Archive_Event.php:109-112, 192-199`)
- `include` sets `post__in`, sets `tribe_remove_date_filters=true` (ignored by the ORM, which only passes it through as a query var), unsets `start_date`/`end_date`, and sets orderby `['date','ID']` with order `ASC` (post publish date, then ID).
- **However**, `parse_args($args, $request->get_default_params())` on line 199 **re-adds the registered defaults** for `start_date` (yesterday 23:59) and `end_date` (+24 months).
- Those defaults are not passed through inclusive-day processing. They are removed again only by the `relative_dates` logic above.
- Net effect from reading the source (verify empirically): **`include` alone does not bypass the default upcoming window. Included events outside [yesterday 23:59 local, now+24 months] are filtered out. A user-supplied `start_date`/`end_date` is also replaced by the default when `include` is used.** Workaround: add a relative param, e.g. `include=1,2,3&starts_after=1970-01-01` (inclusive mode makes that `> 1970-01-01 23:59:59`), so the default window is removed.

#### 7.2.3 Other always-on query behaviour
- **Hidden events are excluded:** `getEvents` calls `$event_orm->by('hidden', false)`, i.e. `meta_not_exists _EventHideFromUpcoming` (`src/Tribe/Query.php:226, 363-371`; `Repositories/Event.php:862-864`). Events with `hide_from_listings: true` never appear in `/events`, but are readable via `/events/{id}`.
- Default ordering: `orderby=event_date`, `order=ASC`, i.e. by start date (`src/Tribe/Query.php:167-172`). This uses `_EventStartDateUTC` in site-tz mode (`Repositories/Event.php:1748-1790`). Tie order is unspecified.
- Status default and readability constraint: see §2.1.
- **Pagination:**
  - `total` = `tribe_get_events(... 'found_posts'=>true)` (`Archive_Event.php:683-691`).
  - `total_pages = ceil(total/per_page)`, or 0 (`Archive_Base.php:113-120`).
  - `next_rest_url` is present iff a query for page+1 returns ≥ 1 id (`has_next`, 427-437).
  - `previous_rest_url` is present iff page ≠ 1 and the page−1 query is non-empty (447-457).
  - **Page beyond the last: HTTP 404 `event-archive-page-not-found`** when the result is empty **and** `page > 1` (271-275). `page=1` with no results returns 200 with `events: []`, `total: 0`, `total_pages: 0`.
- Events the user cannot read are skipped after the query (285-288); this should already be enforced in SQL.
- Responses are cached per user and args for the current request only (non-persistent `Tribe__Cache`).
- Filters: `tribe_events_archive_get_args`, `tribe_rest_events_archive_data`.

### 7.3 `GET /events/{id}` and `GET /events/by-slug/{slug}`
- Args (by id, `Single_Event.php:285-302`): `id` (path, integer, required, `is_event_id_format`: numeric and non-empty) and `password` (string, optional, `is_string`, **empty value gives 400**).
- Args (by slug, `Single_Event_Slug.php:134-144`): `slug` (path, string, required, `is_event_slug`: `get_page_by_path(slug, OBJECT, 'tribe_events')` must exist). **An unknown slug gives 400 `rest_invalid_param`, not 404.**
- Handler order (`Single_Event.php:145-175`):
  1. No post → **404 `rest-event-not-found`**.
  2. Post of another type → **400 `rest-invalid-event-id`**.
  3. `post_status == 'trash'` → **403 `event-no-longer-available`**.
  4. Not `publish` and no `read_post` → **403 `event-not-accessible`**.
  5. Password required and not satisfied → **403 `event-password-protected`**.
  6. 200 with the Event (context `'single'`, so `json_ld` is present, and embedded venues and organizers also carry `json_ld`).

### 7.4 `POST /events` (create)
Permission: `edit_posts` on `tribe_events`. Body params (form-encoded or JSON) (`Single_Event.php:310-463`):

| Name | WP type | Required | Default | Validation | Maps to |
|---|---|---|---|---|---|
| `author` | integer | no | — | `is_user_id` | `post_author`. Silently replaced with the current user if the caller lacks `edit_others_posts` (`Base.php:182-192`). |
| `date` | string | no | — | `is_time` | `post_date`, parsed in the `timezone` param or site tz (`Date_Utils::build_date_object`) |
| `date_utc` | string | no | — | `is_time` | `post_date_gmt` (`localize_date(…,'UTC')`) |
| `title` | string | **yes** | — | `is_string` (non-empty) | `post_title` |
| `description` | string | no | — | `is_string` (empty gives 400) | `post_content` |
| `slug` | string | no | — | `is_string` | `post_name` |
| `excerpt` | string | no | — | `is_string` | `post_excerpt` |
| `status` | string | no | — | `is_post_status` (any registered status) | scaled back (`Base.php:154-166`): with `publish_posts` cap, the given value or `publish`. Without it, `publish`/`future` become `pending`, and absent becomes `draft`. |
| `timezone` | string | no | — | `is_timezone_or_empty` (valid PHP tz id or UTC-offset form per `Tribe__Timezones::is_valid_timezone`) | `EventTimezone`. Also the zone used to interpret `start_date`, `end_date`, `date`. Empty means the site tz. |
| `all_day` | boolean | no | **`false`** | — | `EventAllDay` |
| `start_date` | string | **yes** | — | `is_time` | `build_date_object(start_date, timezone)` → `EventStartDate` (`Y-m-d`) + `EventStartTime` (`H:i:s`) (`Single_Event.php:700-709`) |
| `end_date` | string | **yes** | — | `is_time` | `EventEndDate` + `EventEndTime` |
| `image` | string | no | — | `is_image`: numeric attachment id that is an image, **or** a URL (only for users with `upload_files`; the server issues an HTTP HEAD, which must return 200 with an allowed `image/*` content type) | `FeaturedImage` via `tribe_upload_image` (sideloads URLs) |
| `cost` | (none; string) | no | — | none | `EventCost` (stored raw). `EventCurrencySymbol`/`EventCurrencyPosition` are parsed from it, e.g. `"$10"` gives symbol `$`, position `prefix`. |
| `website` | (none; string) | no | `null` | `is_url_or_empty` (`FILTER_VALIDATE_URL`) | `EventURL` (`FILTER_SANITIZE_URL`) |
| `show_map` | boolean | no | — | — | `EventShowMap` (absent means `false`) |
| `show_map_link` | boolean | no | — | — | `EventShowMapLink` (absent means `false`) |
| `hide_from_listings` | boolean | no | — | — | `EventHideFromUpcoming='yes'` when true |
| `sticky` | boolean | no | — | — | `EventShowInCalendar` |
| `featured` | boolean | no | — | — | `feature_event` |
| `categories` | (none: array or CSV of ids or names) | no | `null` | none | `tax_input[tribe_events_cat] = Tribe__Terms::translate_terms_to_ids(...)`. Unknown names may create terms. An empty array clears them. |
| `tags` | (none) | no | `null` | none | `tax_input[post_tag]`, same rules |
| `venue` | (none) | no | `null` | `is_venue_id_or_entry_or_empty` | see below |
| `organizer` | (none) | no | `null` | `is_organizer_id_or_entry_or_empty` | see below |

**Linked posts in event create/update:**
- `venue` (`Linked_Post_Base::insert`, `Linked_Post_Base.php:42-75`). Accepts **one** of:
  - an existing venue id (`12` or `"12"`);
  - `{"id": 12}`;
  - a venue-create object (fields per §7.8, `venue` name required). This creates a venue. Duplicates are avoided by default (`tribe_events_rest_venue_insert_avoid_duplicates`).
  - The validator also accepts a CSV of venue ids, but `insert()` handles only a single id. Lists are **not** supported for venues.
- `organizer` (`Single_Organizer::insert`, `Single_Organizer.php:105-120`): the value is cast to an array and **each element** is inserted. Send a **JSON array** whose elements are an organizer id, `{"id": n}`, or an organizer-create object (`organizer` name required). A bare scalar id also works. A **bare single object is iterated by its values and breaks**: wrap it in an array.
- A bad `{"id": X}` gives 400 `invalid-linked-post-id`. A failed nested create gives 400 `could-not-create-venue` / `could-not-create-organizer`.
- Omitting or emptying `venue`/`organizer` leaves links unchanged (`false` is passed to `Linked_Posts::handle_submission`, which bails on non-arrays, `src/Tribe/Linked_Posts.php:920-926`). **There is no way to unlink via this API.**

Response: **201** with the Event object (context `''`, so no `json_ld`) (`Single_Event.php:493-497`). Errors: §6. `wp_insert_post` errors give 500.

### 7.5 `POST|PUT|PATCH /events/{id}` and `/events/by-slug/{slug}` (update)
- Args: READ args (`id` + `password`, or `slug`) merged with CREATE args, all marked not required (`Single_Event.php:646-652`). Defaults still apply (`all_day=false`, etc.).
- Handler: 404 `rest-event-not-found` / 400 `rest-invalid-event-id` as for GET (there is **no** trash or access check beyond the permission callback) → `prepare_postarr` → `Tribe__Events__API::updateEvent`. A `false` return gives **403 `could-not-update-event`**. Success gives 200 with the Event (context `''`).
- **Destructive defaults on update (PUT and PATCH behave the same; there is no true partial update):**
  - `all_day` has default `false`, so an omitted value **resets** all-day to false.
  - `show_map`, `show_map_link`, `sticky`, `featured`: `tribe_is_truthy(null)` is `false`, so omitted values are **reset to false** (`Single_Event.php:778-784`).
  - `hide_from_listings`: omitted means `EventHideFromUpcoming` is not set, so `saveEventMeta` **deletes** the hidden flag (`src/Tribe/API.php:186-188`).
  - `status`: omitted gives `scale_back_post_status(null)`, which is **`publish`** for users with `publish_posts`. **Updating a draft without `status` publishes it.**
  - `image`: omitted gives `FeaturedImage = false` (not null, so it is kept), and `saveEventMeta` **deletes the featured image** (`src/Tribe/API.php:206-213`).
  - `start_date`/`end_date` omitted: the dates are not sent (null) and are kept.
  - `title`, `description`, `excerpt`, `slug`, `cost`, `website`, `timezone` omitted: null, so they are not sent and kept.
  - `categories`/`tags` omitted: kept. `[]` clears them.
- **A client doing an "update" should send the full desired state.**

### 7.6 `DELETE /events/{id}` and `/events/by-slug/{slug}`
- Args: READ args. Flow:
  1. 404 or 400 as above.
  2. Already in trash → **410 `event-is-in-trash`**.
  3. The `tribe_events_rest_event_delete` filter may override, else `wp_trash_post`. A `false` result gives 500 `could-not-delete-event`.
- Success: **200** with the Event data after trashing (`status: "trash"`, slug may carry `__trashed`). This always **trashes**; there is no `force` param.
- If the site has `EMPTY_TRASH_DAYS=0`, WP hard-deletes, and the follow-up read returns `event-not-found` with HTTP 500.

### 7.7 `GET /venues` and `GET /organizers`
Args (`Archive_Venue.php:245-293`; organizers identical):

| Name | Type | Default | Validation | Semantics |
|---|---|---|---|---|
| `page` | integer | 1 | `is_positive_int` | |
| `per_page` | integer | site default | `is_positive_int`; clamped at 50 | |
| `search` | string | — | `is_string` | `Tribe__Events__Venue::find_like()` / organizer equivalent: title, description and custom-field LIKE. Converted to `post__in` (no match gives `[0]`). |
| `event` | integer | — | `is_event_id` (+ handler check) | only venues/organizers linked to this event |
| `has_events` | (none) | — | — | `tribe_is_truthy`: truthy means only those with events (draft/pending events are not counted). **Falsy (e.g. `0`) means only those WITHOUT events.** |
| `only_with_upcoming` | (none; `swagger_type` boolean) | `false` | — | `tribe_is_truthy`: only those with upcoming events |
| `status` | (none) | (`publish` for anonymous, `any` for editors) | `filter_post_status_list` | |
| `password`, `context` | undeclared | | | password access (§3.6) |

- Ordering: WP_Query default, **`post_date DESC`** (`src/functions/template-tags/venue.php:813-841`). There are no order params.
- Pagination: as for events. **404 `venue-archive-page-not-found` / `organizer-archive-page-not-found`** when the page is empty and `page > 1`. `total` comes from a `found_posts` count (`Archive_Venue.php:321-334`).
- Items use the venue/organizer objects with context `''` (no `json_ld`).

### 7.8 Venue writes
- **Create** `POST /venues` (`Single_Venue.php:252-376`). All args below are strings unless noted.
  - `venue` (**required**, `is_string`).
  - `author` (integer, `is_user_id`, scaled back).
  - `date`, `date_utc` (`is_time`). `date` uses `Date_Utils::reformat` in the site tz.
  - `description`, `address`, `city`, `country`, `province`, `state`, `zip`, `phone`, `stateprovince`: `is_string_or_empty`, default `null`.
  - `status` (`is_post_status`, scaled back using the **event** post type's `publish_posts` cap, a source quirk).
  - `show_map`, `show_map_link`: type **string**, default `null`, stored as `'1'` or `'false'` via `tribe_is_truthy`.
  - `website` (`is_url_or_empty`).
  - `image` (`is_image_or_empty`).
  - Response **201** with the Venue (single object). Failure gives 400 `could-not-create-venue`. With duplicate avoidance (default on) an **existing matching venue's id may be returned** instead of a new one.
- **Update** `POST|PUT|PATCH /venues/{id}` or by-slug: all args optional. Omitted fields are null and not sent, except:
  - `status`: omitted becomes **`publish`** (scale-back).
  - `image`: omitted gives `FeaturedImage=false`, which **removes the image** (`Venue::save_meta`, `src/Tribe/Venue.php:481-487`).
  - Failure gives 400 `could-not-update-venue`. Success gives 200 with the Venue.
- **Delete**: 410 `venue-is-in-trash` if already trashed. 500 `could-not-delete-venue`. 200 with the Venue data after trash (`status: "trash"`). Filter: `tribe_venues_rest_venue_delete`.
- **GET** `/venues/{id}`: validator `is_venue_id` (numeric and a `tribe_venue` post, any status), so an unknown id gives **400**. Not `publish` and no `read_post` gives 403 `venue-not-accessible` (trashed venues are not specially handled). Success gives 200 with the Venue (context `'single'`, with `json_ld`).

### 7.9 Organizer writes
- **Create** `POST /organizers` (`Single_Organizer.php:245-320`):
  - `organizer` (**required**, `is_string`).
  - `author`, `date`, `date_utc`, `description`, `status`.
  - `phone`, `email`: `is_string_or_empty`. **No email-format validation.**
  - `website` (`is_url_or_empty`), `image` (`is_image_or_empty`).
  - All default `null`.
  - Response 201. Failure gives 400 `could-not-create-organizer`.
- **Update/Delete/GET**: the same patterns and quirks as venues, with the `organizer-*` codes. Filter: `tribe_organizers_rest_organizer_delete`.

### 7.10 `GET /categories` and `GET /tags` (Term_Archive_Base)
Args (`Archive_Category.php:234-315`; tags `Archive_Tag.php:108-183`, which has no `parent`):

| Name | Type | Default | Validation | Passed to WP terms query as |
|---|---|---|---|---|
| `page` | integer | 1 | `is_positive_int` | offset `(page-1)*per_page` |
| `per_page` | integer | site default | `is_positive_int`; clamp 50 | `number` |
| `search` | string | — | `is_string` | `search` |
| `exclude` | (none: CSV or array) | — | every item must exist in the taxonomy (`is_event_category` / `is_post_tag`) | `exclude` |
| `include` | (none) | — | same | `include` |
| `order` | string | — | `is_string` | `order` (`asc`/`desc`; get_terms falls back to ASC on anything else) |
| `orderby` | string | — | `is_string` (no enum check) | `orderby` (get_terms default `name`) |
| `hide_empty` | boolean | **`'1'` → `true`** | WP boolean | `hide_empty`: **empty terms are hidden by default** |
| `parent` | integer | — | `is_event_category` | `parent` (categories only) |
| `post` | integer | — | `is_event_id` (+ handler check) | terms of that event |
| `event` | integer | — | `is_event_id` | alias of `post` |
| `slug` | string | — | `is_string` | `slug` |

- Handler: the params are copied onto the request and **[WP core]** `WP_REST_Terms_Controller::get_items()` is called directly. Core's own permission check is bypassed.
- **`/tags` lists every `post_tag` term on the site** (shared with blog posts). `count` covers all post types.
- Pagination: `total`/`total_pages` come from the core headers. **There is no 404 for out-of-range pages**: you get 200 with an empty list. `has_next = page < total_pages`, `has_previous = page > 1` (`Term_Archive_Base.php:92-102`).

### 7.11 `GET|POST|PUT|PATCH|DELETE /categories/{id}`, `/tags/{id}`; `POST /categories`, `POST /tags`
- `id` (path, integer, required): `is_event_category` / `is_post_tag` (`Single_Category.php:158-168`). An unknown id gives **400**.
- GET: core `get_item`, then a Term (§4.7.2), 200.
- CREATE args (`Single_Category.php:19-45`; tags without `parent`):
  - `name` (string, **required**, `is_string`).
  - `description` (string, `is_string`, so **an empty value gives 400**).
  - `slug` (`is_string`).
  - `parent` (integer, `is_event_category`).
- Create: core `create_item` gives **201** (with a `Location` header) and a Term. A duplicate name gives 400 `term_exists` with `data.term_id`.
- Update: core `update_item` gives 200 and a Term.
- Delete (`Term_Single_Base.php:211-237`): when `tribe_events_rest_term_allow_delete` is true (the default), `force=true` is set and the term is **permanently deleted**. The response is 200 with the **previous** term data as a Term (core's `{deleted, previous}` wrapper is unwrapped). If the filter is false, core returns 501 `rest_trash_not_supported`.
- Permission for all writes: `edit_posts` on events **and** `manage_terms` on the taxonomy.

---------------------------------------------------------------------------------------------------

## 8. Rust typing cheat-sheet

```rust
// Lenient numeric ids that may arrive as "123" or 123
type IdStr = StringOrInt;                     // Event.author, Venue.author, Organizer.author

struct Event {
    id: u64,
    global_id: Option<String>,                // practically always Some; type allows `false`, so use a bool-or-string decoder
    global_id_lineage: Vec<String>,
    author: StringOrInt,
    status: String,                           // open set: publish|future|draft|pending|private|trash|... (custom stati possible)
    date: String, date_utc: String, modified: String, modified_utc: String,   // "Y-m-d H:i:s", may be "0000-00-00 00:00:00"
    url: String, rest_url: String,
    title: String, description: String, excerpt: String, slug: String,        // HTML / entities
    image: FalseOr<Image>,                    // `false` or object
    all_day: Option<bool>,                    // null when password-blanked
    start_date: String, start_date_details: DateDetails,
    end_date: String,   end_date_details: DateDetails,
    utc_start_date: String, utc_start_date_details: DateDetails,
    utc_end_date: String,   utc_end_date_details: DateDetails,
    timezone: String,                         // "" | IANA | "UTC+5.5"-style
    timezone_abbr: String,
    cost: String,
    cost_details: CostDetails,
    website: String,
    show_map: Option<bool>, show_map_link: Option<bool>, hide_from_listings: Option<bool>,
    sticky: Option<bool>, featured: Option<bool>,
    categories: Vec<Term>, tags: Vec<Term>,   // defensively also accept a map
    venue: OneOrMany<Venue>,                  // [] | {..} | [{..},..]
    organizer: Vec<Organizer>,
    custom_fields: EmptyArrayOr<HashMap<String, CustomField>>,  // [] or {...}
    json_ld: Option<serde_json::Value>,       // single GET only
    // plugin extras (ticketed, attendance, is_virtual, ...): Option<...> or #[serde(flatten)] extra
}
struct DateDetails { year: String, month: String, day: String, hour: String, minutes: String, seconds: String }
struct CostDetails { currency_symbol: String, currency_code: String, currency_position: String, values: Vec<StringOrNumber> }
struct Image { url: FalseOr<String>, id: u64, extension: Option<String>, width: Option<u32>, height: Option<u32>,
               filesize: Option<u64>, original_image: Option<String>, sizes: Option<HashMap<String, ImageSize>> }
struct ImageSize { width: u32, height: u32, #[serde(rename="mime-type")] mime_type: String, filesize: Option<u64>, url: String }
struct Venue { id: u64, author: Option<StringOrInt>, status: Option<String>, /* every string field Option */,
               venue: Option<String>, geo_lat: Option<f64>, geo_lng: Option<f64>,
               show_map: Option<bool>, show_map_link: Option<bool>, image: Option<FalseOr<Image>>,
               global_id: Option<String>, global_id_lineage: Option<Vec<String>>, json_ld: Option<Value> }
struct Organizer { id: u64, organizer: Option<String>, phone/website/email: Option<String>, image: Option<FalseOr<Image>>, ... }
struct Term { id: u64, name: String, slug: String, taxonomy: String, description: String,
              parent: Option<u64>, count: u64, term_group: Option<i64>, term_taxonomy_id: Option<u64>,
              filter: Option<String>, url: Option<String>, meta: Option<Value>, urls: TermUrls }
struct TermUrls { #[serde(rename="self")] self_: String, collection: String, up: Option<String> }
struct EventsPage { events: Vec<Event>, rest_url: String, next_rest_url: Option<String>, previous_rest_url: Option<String>, total: u64, total_pages: u64 }
// VenuesPage{venues}, OrganizersPage{organizers}, CategoriesPage{categories}, TagsPage{tags}: same envelope
struct ApiError { code: String, message: String, data: Option<Value> /* {status, params?, details?} */ }
```
Other client guidance:
- Do not use `deny_unknown_fields`.
- Send booleans as `true`/`false` or `1`/`0`.
- Never send empty-string params.
- Build pagination URLs yourself.
- Treat a 404 with code `*-archive-page-not-found` as end-of-data, not as a hard error.

---------------------------------------------------------------------------------------------------

## 9. Items to verify empirically (source-derived, with some uncertainty)
1. `include` + the default date window re-applied by `parse_args` (§7.2.2).
2. **[WP core]** details: the `details` key in `rest_invalid_param`; no 404 on out-of-range term pages; `X-WP-Total` header value types.
3. Whether add-ons (ECP, ET, Virtual) add more Event keys on your target sites.
4. Exact `rest_url` encoding (raw spaces), which depends on the WP version of `add_query_arg`/`build_query`.
5. Venue/organizer update removing the featured image when `image` is omitted. This requires `Venue::update` to reach `save_meta` with `FeaturedImage=false`. Confirmed for events via `API::saveEventMeta`. The same pattern exists in `src/Tribe/Venue.php:481-487` and `src/Tribe/Organizer.php:363-369`.
