// Hooks for the runtime's `script_assets.rs` (see ../hooks.rs).
&[
    // A script's request for a sound bank, and the bank's loading status
    // as the request reads it: a bank this game data lacks counts as loaded.
    ("script_bank_request", "audScriptAudioEntity::RequestScriptBank(unsigned int, bool, unsigned int)"),
    ("bank_loading_status", "rage::audWaveSlot::GetBankLoadingStatus(unsigned int) const"),
    // The same request by the bank's name (REQUEST_MISSION_AUDIO_BANK...):
    // a name the sound factory does not know counts as loaded too.
    ("script_named_bank_request", "audScriptAudioEntity::RequestScriptBank(char const*, bool, unsigned int, bool)"),
    ("bank_index_from_name", "rage::audSoundFactory::GetBankIndexFromName(char const*) const"),
    // HAS_STREAMED_TEXTURE_DICT_LOADED: a dictionary this game data lacks
    // counts as loaded. The hash names the dictionary in the store.
    ("script_txd_loaded", "graphics_commands::HasStreamedTxdLoaded(char const*)"),
    ("hash_string", "rage::atHashStringNamespaceSupport::ComputeHash(rage::atHashStringNamespaces, char const*)"),
    // REQUEST_SCRIPT and HAS_SCRIPT_LOADED: scripts for places the game
    // data lacks are never loaded.
    ("script_request", "script_commands::RequestScript(char const*)"),
    ("script_loaded", "script_commands::HasScriptLoaded(char const*)"),
    // HAS_MODEL_LOADED: a model this game data lacks counts as loaded (its
    // archetype is looked up by hash).
    ("script_model_loaded", "streaming_commands::HasModelLoaded(int)"),
    ("archetype_from_hash", "rage::fwArchetypeManager::GetArchetypeFromHashKey(unsigned int, unsigned int*)"),
    // CREATE_OBJECT and CREATE_OBJECT_NO_OFFSET: a model this game data
    // lacks makes a hidden stand-in, its visibility and collision turned off.
    ("create_object", "object_commands::SetupScriptCommands()::scrWrapped_CREATE_OBJECT::Call(rage::scrThread::Info&)"),
    ("create_object_no_offset", "object_commands::SetupScriptCommands()::scrWrapped_CREATE_OBJECT_NO_OFFSET::Call(rage::scrThread::Info&)"),
    ("set_entity_visible", "entity_commands::CommandSetEntityVisible(int, bool, bool)"),
    ("set_entity_collision", "entity_commands::CommandSetEntityCollision(int, bool, bool)"),
    // GET_INTERIOR_AT_COORDS_WITH_TYPE: for a type this game data lacks, the
    // interior at those coordinates (an earlier version of the place).
    ("interior_of_type", "interior_commands::CommandGetInteriorAtCoordsOfType(rage::scrVector const&, char const*)"),
    ("interior_at", "interior_commands::CommandGetInteriorAtCoords(rage::scrVector const&)"),
    // HAS_THIS_ADDITIONAL_TEXT_LOADED: a text block asked for whose load
    // ended without it (this data lacks it) counts as loaded. The requests
    // and freed slots say what each slot was asked for.
    ("additional_text_loaded", "CTextFile::HasThisAdditionalTextLoaded(char const*, int)"),
    ("additional_text_requesting", "CTextFile::IsRequestingThisAdditionalText(char const*, int)"),
    ("text_request", "CTextFile::RequestAdditionalText(char const*, int, CTextFile::eLocationOfTextBlock)"),
    ("text_free", "CTextFile::FreeTextSlot(int, bool)"),
    // A function of a movie's ActionScript: calls nest at most so deep (the
    // phone's movie calls itself without end for an app it lacks).
    ("as_function_invoke", "GASAsFunctionObject::Invoke(GASFnCall const&, GASLocalFrame*, char const*)"),
    ("user_function_invoke", "GASUserDefinedFunctionObject::Invoke(GASFnCall const&, GASLocalFrame*, char const*)"),
    // Scaleform methods being built and their numbers: the phone's home
    // screen gets no app its movie lacks.
    ("method_begin", "CScaleformMgr::BeginMethodInternal(int, eSCALEFORM_BASE_CLASS, char const*, int, int)"),
    ("param_int", "CScaleformMgr::AddParamInt(int)"),
]
