// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'app.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$SessionEvent {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SessionEvent);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'SessionEvent()';
}


}

/// @nodoc
class $SessionEventCopyWith<$Res>  {
$SessionEventCopyWith(SessionEvent _, $Res Function(SessionEvent) __);
}


/// Adds pattern-matching-related methods to [SessionEvent].
extension SessionEventPatterns on SessionEvent {
/// A variant of `map` that fallback to returning `orElse`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( SessionEvent_Opened value)?  opened,TResult Function( SessionEvent_Connected value)?  connected,TResult Function( SessionEvent_Output value)?  output,TResult Function( SessionEvent_HostKey value)?  hostKey,TResult Function( SessionEvent_Password value)?  password,TResult Function( SessionEvent_ServerPrompts value)?  serverPrompts,TResult Function( SessionEvent_Exit value)?  exit,TResult Function( SessionEvent_Failed value)?  failed,TResult Function( SessionEvent_Closed value)?  closed,required TResult orElse(),}){
final _that = this;
switch (_that) {
case SessionEvent_Opened() when opened != null:
return opened(_that);case SessionEvent_Connected() when connected != null:
return connected(_that);case SessionEvent_Output() when output != null:
return output(_that);case SessionEvent_HostKey() when hostKey != null:
return hostKey(_that);case SessionEvent_Password() when password != null:
return password(_that);case SessionEvent_ServerPrompts() when serverPrompts != null:
return serverPrompts(_that);case SessionEvent_Exit() when exit != null:
return exit(_that);case SessionEvent_Failed() when failed != null:
return failed(_that);case SessionEvent_Closed() when closed != null:
return closed(_that);case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// Callbacks receives the raw object, upcasted.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case final Subclass2 value:
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( SessionEvent_Opened value)  opened,required TResult Function( SessionEvent_Connected value)  connected,required TResult Function( SessionEvent_Output value)  output,required TResult Function( SessionEvent_HostKey value)  hostKey,required TResult Function( SessionEvent_Password value)  password,required TResult Function( SessionEvent_ServerPrompts value)  serverPrompts,required TResult Function( SessionEvent_Exit value)  exit,required TResult Function( SessionEvent_Failed value)  failed,required TResult Function( SessionEvent_Closed value)  closed,}){
final _that = this;
switch (_that) {
case SessionEvent_Opened():
return opened(_that);case SessionEvent_Connected():
return connected(_that);case SessionEvent_Output():
return output(_that);case SessionEvent_HostKey():
return hostKey(_that);case SessionEvent_Password():
return password(_that);case SessionEvent_ServerPrompts():
return serverPrompts(_that);case SessionEvent_Exit():
return exit(_that);case SessionEvent_Failed():
return failed(_that);case SessionEvent_Closed():
return closed(_that);}
}
/// A variant of `map` that fallback to returning `null`.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case final Subclass value:
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( SessionEvent_Opened value)?  opened,TResult? Function( SessionEvent_Connected value)?  connected,TResult? Function( SessionEvent_Output value)?  output,TResult? Function( SessionEvent_HostKey value)?  hostKey,TResult? Function( SessionEvent_Password value)?  password,TResult? Function( SessionEvent_ServerPrompts value)?  serverPrompts,TResult? Function( SessionEvent_Exit value)?  exit,TResult? Function( SessionEvent_Failed value)?  failed,TResult? Function( SessionEvent_Closed value)?  closed,}){
final _that = this;
switch (_that) {
case SessionEvent_Opened() when opened != null:
return opened(_that);case SessionEvent_Connected() when connected != null:
return connected(_that);case SessionEvent_Output() when output != null:
return output(_that);case SessionEvent_HostKey() when hostKey != null:
return hostKey(_that);case SessionEvent_Password() when password != null:
return password(_that);case SessionEvent_ServerPrompts() when serverPrompts != null:
return serverPrompts(_that);case SessionEvent_Exit() when exit != null:
return exit(_that);case SessionEvent_Failed() when failed != null:
return failed(_that);case SessionEvent_Closed() when closed != null:
return closed(_that);case _:
  return null;

}
}
/// A variant of `when` that fallback to an `orElse` callback.
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return orElse();
/// }
/// ```

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function( int sessionId)?  opened,TResult Function( String description)?  connected,TResult Function( Uint8List data)?  output,TResult Function( int requestId,  String host,  int port,  String algorithm,  String fingerprint,  List<String> knownFingerprints)?  hostKey,TResult Function( int requestId,  String prompt)?  password,TResult Function( int requestId,  String name,  String instructions,  List<PromptView> prompts)?  serverPrompts,TResult Function( int? code)?  exit,TResult Function( String message)?  failed,TResult Function()?  closed,required TResult orElse(),}) {final _that = this;
switch (_that) {
case SessionEvent_Opened() when opened != null:
return opened(_that.sessionId);case SessionEvent_Connected() when connected != null:
return connected(_that.description);case SessionEvent_Output() when output != null:
return output(_that.data);case SessionEvent_HostKey() when hostKey != null:
return hostKey(_that.requestId,_that.host,_that.port,_that.algorithm,_that.fingerprint,_that.knownFingerprints);case SessionEvent_Password() when password != null:
return password(_that.requestId,_that.prompt);case SessionEvent_ServerPrompts() when serverPrompts != null:
return serverPrompts(_that.requestId,_that.name,_that.instructions,_that.prompts);case SessionEvent_Exit() when exit != null:
return exit(_that.code);case SessionEvent_Failed() when failed != null:
return failed(_that.message);case SessionEvent_Closed() when closed != null:
return closed();case _:
  return orElse();

}
}
/// A `switch`-like method, using callbacks.
///
/// As opposed to `map`, this offers destructuring.
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case Subclass2(:final field2):
///     return ...;
/// }
/// ```

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function( int sessionId)  opened,required TResult Function( String description)  connected,required TResult Function( Uint8List data)  output,required TResult Function( int requestId,  String host,  int port,  String algorithm,  String fingerprint,  List<String> knownFingerprints)  hostKey,required TResult Function( int requestId,  String prompt)  password,required TResult Function( int requestId,  String name,  String instructions,  List<PromptView> prompts)  serverPrompts,required TResult Function( int? code)  exit,required TResult Function( String message)  failed,required TResult Function()  closed,}) {final _that = this;
switch (_that) {
case SessionEvent_Opened():
return opened(_that.sessionId);case SessionEvent_Connected():
return connected(_that.description);case SessionEvent_Output():
return output(_that.data);case SessionEvent_HostKey():
return hostKey(_that.requestId,_that.host,_that.port,_that.algorithm,_that.fingerprint,_that.knownFingerprints);case SessionEvent_Password():
return password(_that.requestId,_that.prompt);case SessionEvent_ServerPrompts():
return serverPrompts(_that.requestId,_that.name,_that.instructions,_that.prompts);case SessionEvent_Exit():
return exit(_that.code);case SessionEvent_Failed():
return failed(_that.message);case SessionEvent_Closed():
return closed();}
}
/// A variant of `when` that fallback to returning `null`
///
/// It is equivalent to doing:
/// ```dart
/// switch (sealedClass) {
///   case Subclass(:final field):
///     return ...;
///   case _:
///     return null;
/// }
/// ```

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function( int sessionId)?  opened,TResult? Function( String description)?  connected,TResult? Function( Uint8List data)?  output,TResult? Function( int requestId,  String host,  int port,  String algorithm,  String fingerprint,  List<String> knownFingerprints)?  hostKey,TResult? Function( int requestId,  String prompt)?  password,TResult? Function( int requestId,  String name,  String instructions,  List<PromptView> prompts)?  serverPrompts,TResult? Function( int? code)?  exit,TResult? Function( String message)?  failed,TResult? Function()?  closed,}) {final _that = this;
switch (_that) {
case SessionEvent_Opened() when opened != null:
return opened(_that.sessionId);case SessionEvent_Connected() when connected != null:
return connected(_that.description);case SessionEvent_Output() when output != null:
return output(_that.data);case SessionEvent_HostKey() when hostKey != null:
return hostKey(_that.requestId,_that.host,_that.port,_that.algorithm,_that.fingerprint,_that.knownFingerprints);case SessionEvent_Password() when password != null:
return password(_that.requestId,_that.prompt);case SessionEvent_ServerPrompts() when serverPrompts != null:
return serverPrompts(_that.requestId,_that.name,_that.instructions,_that.prompts);case SessionEvent_Exit() when exit != null:
return exit(_that.code);case SessionEvent_Failed() when failed != null:
return failed(_that.message);case SessionEvent_Closed() when closed != null:
return closed();case _:
  return null;

}
}

}

/// @nodoc


class SessionEvent_Opened extends SessionEvent {
  const SessionEvent_Opened({required this.sessionId}): super._();
  

 final  int sessionId;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SessionEvent_OpenedCopyWith<SessionEvent_Opened> get copyWith => _$SessionEvent_OpenedCopyWithImpl<SessionEvent_Opened>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SessionEvent_Opened&&(identical(other.sessionId, sessionId) || other.sessionId == sessionId));
}


@override
int get hashCode {
    return Object.hash(runtimeType,sessionId);
}

@override
String toString() {
    return 'SessionEvent.opened(sessionId: $sessionId)';
}


}

/// @nodoc
abstract mixin class $SessionEvent_OpenedCopyWith<$Res> implements $SessionEventCopyWith<$Res> {
  factory $SessionEvent_OpenedCopyWith(SessionEvent_Opened value, $Res Function(SessionEvent_Opened) _then) = _$SessionEvent_OpenedCopyWithImpl;
@useResult
$Res call({
 int sessionId
});




}
/// @nodoc
class _$SessionEvent_OpenedCopyWithImpl<$Res>
    implements $SessionEvent_OpenedCopyWith<$Res> {
  _$SessionEvent_OpenedCopyWithImpl(this._self, this._then);

  final SessionEvent_Opened _self;
  final $Res Function(SessionEvent_Opened) _then;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? sessionId = null,}) {
  return _then(SessionEvent_Opened(
sessionId: null == sessionId ? _self.sessionId : sessionId // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class SessionEvent_Connected extends SessionEvent {
  const SessionEvent_Connected({required this.description}): super._();
  

 final  String description;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SessionEvent_ConnectedCopyWith<SessionEvent_Connected> get copyWith => _$SessionEvent_ConnectedCopyWithImpl<SessionEvent_Connected>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SessionEvent_Connected&&(identical(other.description, description) || other.description == description));
}


@override
int get hashCode {
    return Object.hash(runtimeType,description);
}

@override
String toString() {
    return 'SessionEvent.connected(description: $description)';
}


}

/// @nodoc
abstract mixin class $SessionEvent_ConnectedCopyWith<$Res> implements $SessionEventCopyWith<$Res> {
  factory $SessionEvent_ConnectedCopyWith(SessionEvent_Connected value, $Res Function(SessionEvent_Connected) _then) = _$SessionEvent_ConnectedCopyWithImpl;
@useResult
$Res call({
 String description
});




}
/// @nodoc
class _$SessionEvent_ConnectedCopyWithImpl<$Res>
    implements $SessionEvent_ConnectedCopyWith<$Res> {
  _$SessionEvent_ConnectedCopyWithImpl(this._self, this._then);

  final SessionEvent_Connected _self;
  final $Res Function(SessionEvent_Connected) _then;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? description = null,}) {
  return _then(SessionEvent_Connected(
description: null == description ? _self.description : description // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class SessionEvent_Output extends SessionEvent {
  const SessionEvent_Output({required this.data}): super._();
  

 final  Uint8List data;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SessionEvent_OutputCopyWith<SessionEvent_Output> get copyWith => _$SessionEvent_OutputCopyWithImpl<SessionEvent_Output>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SessionEvent_Output&&const DeepCollectionEquality().equals(other.data, data));
}


@override
int get hashCode {
    return Object.hash(runtimeType,const DeepCollectionEquality().hash(data));
}

@override
String toString() {
    return 'SessionEvent.output(data: $data)';
}


}

/// @nodoc
abstract mixin class $SessionEvent_OutputCopyWith<$Res> implements $SessionEventCopyWith<$Res> {
  factory $SessionEvent_OutputCopyWith(SessionEvent_Output value, $Res Function(SessionEvent_Output) _then) = _$SessionEvent_OutputCopyWithImpl;
@useResult
$Res call({
 Uint8List data
});




}
/// @nodoc
class _$SessionEvent_OutputCopyWithImpl<$Res>
    implements $SessionEvent_OutputCopyWith<$Res> {
  _$SessionEvent_OutputCopyWithImpl(this._self, this._then);

  final SessionEvent_Output _self;
  final $Res Function(SessionEvent_Output) _then;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? data = null,}) {
  return _then(SessionEvent_Output(
data: null == data ? _self.data : data // ignore: cast_nullable_to_non_nullable
as Uint8List,
  ));
}


}

/// @nodoc


class SessionEvent_HostKey extends SessionEvent {
  const SessionEvent_HostKey({required this.requestId, required this.host, required this.port, required this.algorithm, required this.fingerprint, required  List<String> knownFingerprints}): _knownFingerprints = knownFingerprints,super._();
  

 final  int requestId;
 final  String host;
 final  int port;
 final  String algorithm;
 final  String fingerprint;
/// Zuvor bekannte Fingerabdrücke; nicht leer heißt: Der Schlüssel hat sich geändert.
 final  List<String> _knownFingerprints;
/// Zuvor bekannte Fingerabdrücke; nicht leer heißt: Der Schlüssel hat sich geändert.
 List<String> get knownFingerprints {
  if (_knownFingerprints is EqualUnmodifiableListView) return _knownFingerprints;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_knownFingerprints);
}


/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SessionEvent_HostKeyCopyWith<SessionEvent_HostKey> get copyWith => _$SessionEvent_HostKeyCopyWithImpl<SessionEvent_HostKey>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SessionEvent_HostKey&&(identical(other.requestId, requestId) || other.requestId == requestId)&&(identical(other.host, host) || other.host == host)&&(identical(other.port, port) || other.port == port)&&(identical(other.algorithm, algorithm) || other.algorithm == algorithm)&&(identical(other.fingerprint, fingerprint) || other.fingerprint == fingerprint)&&const DeepCollectionEquality().equals(other.knownFingerprints, _knownFingerprints));
}


@override
int get hashCode {
    return Object.hash(runtimeType,requestId,host,port,algorithm,fingerprint,const DeepCollectionEquality().hash(_knownFingerprints));
}

@override
String toString() {
    return 'SessionEvent.hostKey(requestId: $requestId, host: $host, port: $port, algorithm: $algorithm, fingerprint: $fingerprint, knownFingerprints: $knownFingerprints)';
}


}

/// @nodoc
abstract mixin class $SessionEvent_HostKeyCopyWith<$Res> implements $SessionEventCopyWith<$Res> {
  factory $SessionEvent_HostKeyCopyWith(SessionEvent_HostKey value, $Res Function(SessionEvent_HostKey) _then) = _$SessionEvent_HostKeyCopyWithImpl;
@useResult
$Res call({
 int requestId, String host, int port, String algorithm, String fingerprint, List<String> knownFingerprints
});




}
/// @nodoc
class _$SessionEvent_HostKeyCopyWithImpl<$Res>
    implements $SessionEvent_HostKeyCopyWith<$Res> {
  _$SessionEvent_HostKeyCopyWithImpl(this._self, this._then);

  final SessionEvent_HostKey _self;
  final $Res Function(SessionEvent_HostKey) _then;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? requestId = null,Object? host = null,Object? port = null,Object? algorithm = null,Object? fingerprint = null,Object? knownFingerprints = null,}) {
  return _then(SessionEvent_HostKey(
requestId: null == requestId ? _self.requestId : requestId // ignore: cast_nullable_to_non_nullable
as int,host: null == host ? _self.host : host // ignore: cast_nullable_to_non_nullable
as String,port: null == port ? _self.port : port // ignore: cast_nullable_to_non_nullable
as int,algorithm: null == algorithm ? _self.algorithm : algorithm // ignore: cast_nullable_to_non_nullable
as String,fingerprint: null == fingerprint ? _self.fingerprint : fingerprint // ignore: cast_nullable_to_non_nullable
as String,knownFingerprints: null == knownFingerprints ? _self._knownFingerprints : knownFingerprints // ignore: cast_nullable_to_non_nullable
as List<String>,
  ));
}


}

/// @nodoc


class SessionEvent_Password extends SessionEvent {
  const SessionEvent_Password({required this.requestId, required this.prompt}): super._();
  

 final  int requestId;
 final  String prompt;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SessionEvent_PasswordCopyWith<SessionEvent_Password> get copyWith => _$SessionEvent_PasswordCopyWithImpl<SessionEvent_Password>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SessionEvent_Password&&(identical(other.requestId, requestId) || other.requestId == requestId)&&(identical(other.prompt, prompt) || other.prompt == prompt));
}


@override
int get hashCode {
    return Object.hash(runtimeType,requestId,prompt);
}

@override
String toString() {
    return 'SessionEvent.password(requestId: $requestId, prompt: $prompt)';
}


}

/// @nodoc
abstract mixin class $SessionEvent_PasswordCopyWith<$Res> implements $SessionEventCopyWith<$Res> {
  factory $SessionEvent_PasswordCopyWith(SessionEvent_Password value, $Res Function(SessionEvent_Password) _then) = _$SessionEvent_PasswordCopyWithImpl;
@useResult
$Res call({
 int requestId, String prompt
});




}
/// @nodoc
class _$SessionEvent_PasswordCopyWithImpl<$Res>
    implements $SessionEvent_PasswordCopyWith<$Res> {
  _$SessionEvent_PasswordCopyWithImpl(this._self, this._then);

  final SessionEvent_Password _self;
  final $Res Function(SessionEvent_Password) _then;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? requestId = null,Object? prompt = null,}) {
  return _then(SessionEvent_Password(
requestId: null == requestId ? _self.requestId : requestId // ignore: cast_nullable_to_non_nullable
as int,prompt: null == prompt ? _self.prompt : prompt // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class SessionEvent_ServerPrompts extends SessionEvent {
  const SessionEvent_ServerPrompts({required this.requestId, required this.name, required this.instructions, required  List<PromptView> prompts}): _prompts = prompts,super._();
  

 final  int requestId;
 final  String name;
 final  String instructions;
 final  List<PromptView> _prompts;
 List<PromptView> get prompts {
  if (_prompts is EqualUnmodifiableListView) return _prompts;
  // ignore: implicit_dynamic_type
  return EqualUnmodifiableListView(_prompts);
}


/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SessionEvent_ServerPromptsCopyWith<SessionEvent_ServerPrompts> get copyWith => _$SessionEvent_ServerPromptsCopyWithImpl<SessionEvent_ServerPrompts>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SessionEvent_ServerPrompts&&(identical(other.requestId, requestId) || other.requestId == requestId)&&(identical(other.name, name) || other.name == name)&&(identical(other.instructions, instructions) || other.instructions == instructions)&&const DeepCollectionEquality().equals(other.prompts, _prompts));
}


@override
int get hashCode {
    return Object.hash(runtimeType,requestId,name,instructions,const DeepCollectionEquality().hash(_prompts));
}

@override
String toString() {
    return 'SessionEvent.serverPrompts(requestId: $requestId, name: $name, instructions: $instructions, prompts: $prompts)';
}


}

/// @nodoc
abstract mixin class $SessionEvent_ServerPromptsCopyWith<$Res> implements $SessionEventCopyWith<$Res> {
  factory $SessionEvent_ServerPromptsCopyWith(SessionEvent_ServerPrompts value, $Res Function(SessionEvent_ServerPrompts) _then) = _$SessionEvent_ServerPromptsCopyWithImpl;
@useResult
$Res call({
 int requestId, String name, String instructions, List<PromptView> prompts
});




}
/// @nodoc
class _$SessionEvent_ServerPromptsCopyWithImpl<$Res>
    implements $SessionEvent_ServerPromptsCopyWith<$Res> {
  _$SessionEvent_ServerPromptsCopyWithImpl(this._self, this._then);

  final SessionEvent_ServerPrompts _self;
  final $Res Function(SessionEvent_ServerPrompts) _then;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? requestId = null,Object? name = null,Object? instructions = null,Object? prompts = null,}) {
  return _then(SessionEvent_ServerPrompts(
requestId: null == requestId ? _self.requestId : requestId // ignore: cast_nullable_to_non_nullable
as int,name: null == name ? _self.name : name // ignore: cast_nullable_to_non_nullable
as String,instructions: null == instructions ? _self.instructions : instructions // ignore: cast_nullable_to_non_nullable
as String,prompts: null == prompts ? _self._prompts : prompts // ignore: cast_nullable_to_non_nullable
as List<PromptView>,
  ));
}


}

/// @nodoc


class SessionEvent_Exit extends SessionEvent {
  const SessionEvent_Exit({this.code}): super._();
  

 final  int? code;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SessionEvent_ExitCopyWith<SessionEvent_Exit> get copyWith => _$SessionEvent_ExitCopyWithImpl<SessionEvent_Exit>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SessionEvent_Exit&&(identical(other.code, code) || other.code == code));
}


@override
int get hashCode {
    return Object.hash(runtimeType,code);
}

@override
String toString() {
    return 'SessionEvent.exit(code: $code)';
}


}

/// @nodoc
abstract mixin class $SessionEvent_ExitCopyWith<$Res> implements $SessionEventCopyWith<$Res> {
  factory $SessionEvent_ExitCopyWith(SessionEvent_Exit value, $Res Function(SessionEvent_Exit) _then) = _$SessionEvent_ExitCopyWithImpl;
@useResult
$Res call({
 int? code
});




}
/// @nodoc
class _$SessionEvent_ExitCopyWithImpl<$Res>
    implements $SessionEvent_ExitCopyWith<$Res> {
  _$SessionEvent_ExitCopyWithImpl(this._self, this._then);

  final SessionEvent_Exit _self;
  final $Res Function(SessionEvent_Exit) _then;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? code = freezed,}) {
  return _then(SessionEvent_Exit(
code: freezed == code ? _self.code : code // ignore: cast_nullable_to_non_nullable
as int?,
  ));
}


}

/// @nodoc


class SessionEvent_Failed extends SessionEvent {
  const SessionEvent_Failed({required this.message}): super._();
  

 final  String message;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$SessionEvent_FailedCopyWith<SessionEvent_Failed> get copyWith => _$SessionEvent_FailedCopyWithImpl<SessionEvent_Failed>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SessionEvent_Failed&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode {
    return Object.hash(runtimeType,message);
}

@override
String toString() {
    return 'SessionEvent.failed(message: $message)';
}


}

/// @nodoc
abstract mixin class $SessionEvent_FailedCopyWith<$Res> implements $SessionEventCopyWith<$Res> {
  factory $SessionEvent_FailedCopyWith(SessionEvent_Failed value, $Res Function(SessionEvent_Failed) _then) = _$SessionEvent_FailedCopyWithImpl;
@useResult
$Res call({
 String message
});




}
/// @nodoc
class _$SessionEvent_FailedCopyWithImpl<$Res>
    implements $SessionEvent_FailedCopyWith<$Res> {
  _$SessionEvent_FailedCopyWithImpl(this._self, this._then);

  final SessionEvent_Failed _self;
  final $Res Function(SessionEvent_Failed) _then;

/// Create a copy of SessionEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = null,}) {
  return _then(SessionEvent_Failed(
message: null == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class SessionEvent_Closed extends SessionEvent {
  const SessionEvent_Closed(): super._();
  






@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is SessionEvent_Closed);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'SessionEvent.closed()';
}


}




// dart format on
