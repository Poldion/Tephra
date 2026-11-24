// GENERATED CODE - DO NOT MODIFY BY HAND
// coverage:ignore-file
// ignore_for_file: type=lint, type=warning, deprecated_member_use, deprecated_member_use_from_same_package
// ignore_for_file: unused_element, deprecated_member_use, deprecated_member_use_from_same_package, use_function_type_syntax_for_parameters, unnecessary_const, avoid_init_to_null, invalid_override_different_default_values_named, prefer_expression_function_bodies, annotate_overrides, invalid_annotation_target, unnecessary_question_mark

part of 'mcp.dart';

// **************************************************************************
// FreezedGenerator
// **************************************************************************

// GENERATED CODE - DO NOT MODIFY BY HAND
// dart format off
T _$identity<T>(T value) => value;
/// @nodoc
mixin _$McpEvent {





@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is McpEvent);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'McpEvent()';
}


}

/// @nodoc
class $McpEventCopyWith<$Res>  {
$McpEventCopyWith(McpEvent _, $Res Function(McpEvent) __);
}


/// Adds pattern-matching-related methods to [McpEvent].
extension McpEventPatterns on McpEvent {
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

@optionalTypeArgs TResult maybeMap<TResult extends Object?>({TResult Function( McpEvent_Started value)?  started,TResult Function( McpEvent_Request value)?  request,TResult Function( McpEvent_Withdrawn value)?  withdrawn,TResult Function( McpEvent_Connected value)?  connected,TResult Function( McpEvent_Disconnected value)?  disconnected,TResult Function( McpEvent_Stopped value)?  stopped,required TResult orElse(),}){
final _that = this;
switch (_that) {
case McpEvent_Started() when started != null:
return started(_that);case McpEvent_Request() when request != null:
return request(_that);case McpEvent_Withdrawn() when withdrawn != null:
return withdrawn(_that);case McpEvent_Connected() when connected != null:
return connected(_that);case McpEvent_Disconnected() when disconnected != null:
return disconnected(_that);case McpEvent_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult map<TResult extends Object?>({required TResult Function( McpEvent_Started value)  started,required TResult Function( McpEvent_Request value)  request,required TResult Function( McpEvent_Withdrawn value)  withdrawn,required TResult Function( McpEvent_Connected value)  connected,required TResult Function( McpEvent_Disconnected value)  disconnected,required TResult Function( McpEvent_Stopped value)  stopped,}){
final _that = this;
switch (_that) {
case McpEvent_Started():
return started(_that);case McpEvent_Request():
return request(_that);case McpEvent_Withdrawn():
return withdrawn(_that);case McpEvent_Connected():
return connected(_that);case McpEvent_Disconnected():
return disconnected(_that);case McpEvent_Stopped():
return stopped(_that);}
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

@optionalTypeArgs TResult? mapOrNull<TResult extends Object?>({TResult? Function( McpEvent_Started value)?  started,TResult? Function( McpEvent_Request value)?  request,TResult? Function( McpEvent_Withdrawn value)?  withdrawn,TResult? Function( McpEvent_Connected value)?  connected,TResult? Function( McpEvent_Disconnected value)?  disconnected,TResult? Function( McpEvent_Stopped value)?  stopped,}){
final _that = this;
switch (_that) {
case McpEvent_Started() when started != null:
return started(_that);case McpEvent_Request() when request != null:
return request(_that);case McpEvent_Withdrawn() when withdrawn != null:
return withdrawn(_that);case McpEvent_Connected() when connected != null:
return connected(_that);case McpEvent_Disconnected() when disconnected != null:
return disconnected(_that);case McpEvent_Stopped() when stopped != null:
return stopped(_that);case _:
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

@optionalTypeArgs TResult maybeWhen<TResult extends Object?>({TResult Function()?  started,TResult Function( McpRequest request)?  request,TResult Function( int id)?  withdrawn,TResult Function( String client)?  connected,TResult Function( String client)?  disconnected,TResult Function( String? message)?  stopped,required TResult orElse(),}) {final _that = this;
switch (_that) {
case McpEvent_Started() when started != null:
return started();case McpEvent_Request() when request != null:
return request(_that.request);case McpEvent_Withdrawn() when withdrawn != null:
return withdrawn(_that.id);case McpEvent_Connected() when connected != null:
return connected(_that.client);case McpEvent_Disconnected() when disconnected != null:
return disconnected(_that.client);case McpEvent_Stopped() when stopped != null:
return stopped(_that.message);case _:
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

@optionalTypeArgs TResult when<TResult extends Object?>({required TResult Function()  started,required TResult Function( McpRequest request)  request,required TResult Function( int id)  withdrawn,required TResult Function( String client)  connected,required TResult Function( String client)  disconnected,required TResult Function( String? message)  stopped,}) {final _that = this;
switch (_that) {
case McpEvent_Started():
return started();case McpEvent_Request():
return request(_that.request);case McpEvent_Withdrawn():
return withdrawn(_that.id);case McpEvent_Connected():
return connected(_that.client);case McpEvent_Disconnected():
return disconnected(_that.client);case McpEvent_Stopped():
return stopped(_that.message);}
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

@optionalTypeArgs TResult? whenOrNull<TResult extends Object?>({TResult? Function()?  started,TResult? Function( McpRequest request)?  request,TResult? Function( int id)?  withdrawn,TResult? Function( String client)?  connected,TResult? Function( String client)?  disconnected,TResult? Function( String? message)?  stopped,}) {final _that = this;
switch (_that) {
case McpEvent_Started() when started != null:
return started();case McpEvent_Request() when request != null:
return request(_that.request);case McpEvent_Withdrawn() when withdrawn != null:
return withdrawn(_that.id);case McpEvent_Connected() when connected != null:
return connected(_that.client);case McpEvent_Disconnected() when disconnected != null:
return disconnected(_that.client);case McpEvent_Stopped() when stopped != null:
return stopped(_that.message);case _:
  return null;

}
}

}

/// @nodoc


class McpEvent_Started extends McpEvent {
  const McpEvent_Started(): super._();
  






@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is McpEvent_Started);
}


@override
int get hashCode => runtimeType.hashCode;

@override
String toString() {
    return 'McpEvent.started()';
}


}




/// @nodoc


class McpEvent_Request extends McpEvent {
  const McpEvent_Request({required this.request}): super._();
  

 final  McpRequest request;

/// Create a copy of McpEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$McpEvent_RequestCopyWith<McpEvent_Request> get copyWith => _$McpEvent_RequestCopyWithImpl<McpEvent_Request>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is McpEvent_Request&&(identical(other.request, request) || other.request == request));
}


@override
int get hashCode {
    return Object.hash(runtimeType,request);
}

@override
String toString() {
    return 'McpEvent.request(request: $request)';
}


}

/// @nodoc
abstract mixin class $McpEvent_RequestCopyWith<$Res> implements $McpEventCopyWith<$Res> {
  factory $McpEvent_RequestCopyWith(McpEvent_Request value, $Res Function(McpEvent_Request) _then) = _$McpEvent_RequestCopyWithImpl;
@useResult
$Res call({
 McpRequest request
});




}
/// @nodoc
class _$McpEvent_RequestCopyWithImpl<$Res>
    implements $McpEvent_RequestCopyWith<$Res> {
  _$McpEvent_RequestCopyWithImpl(this._self, this._then);

  final McpEvent_Request _self;
  final $Res Function(McpEvent_Request) _then;

/// Create a copy of McpEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? request = null,}) {
  return _then(McpEvent_Request(
request: null == request ? _self.request : request // ignore: cast_nullable_to_non_nullable
as McpRequest,
  ));
}


}

/// @nodoc


class McpEvent_Withdrawn extends McpEvent {
  const McpEvent_Withdrawn({required this.id}): super._();
  

 final  int id;

/// Create a copy of McpEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$McpEvent_WithdrawnCopyWith<McpEvent_Withdrawn> get copyWith => _$McpEvent_WithdrawnCopyWithImpl<McpEvent_Withdrawn>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is McpEvent_Withdrawn&&(identical(other.id, id) || other.id == id));
}


@override
int get hashCode {
    return Object.hash(runtimeType,id);
}

@override
String toString() {
    return 'McpEvent.withdrawn(id: $id)';
}


}

/// @nodoc
abstract mixin class $McpEvent_WithdrawnCopyWith<$Res> implements $McpEventCopyWith<$Res> {
  factory $McpEvent_WithdrawnCopyWith(McpEvent_Withdrawn value, $Res Function(McpEvent_Withdrawn) _then) = _$McpEvent_WithdrawnCopyWithImpl;
@useResult
$Res call({
 int id
});




}
/// @nodoc
class _$McpEvent_WithdrawnCopyWithImpl<$Res>
    implements $McpEvent_WithdrawnCopyWith<$Res> {
  _$McpEvent_WithdrawnCopyWithImpl(this._self, this._then);

  final McpEvent_Withdrawn _self;
  final $Res Function(McpEvent_Withdrawn) _then;

/// Create a copy of McpEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? id = null,}) {
  return _then(McpEvent_Withdrawn(
id: null == id ? _self.id : id // ignore: cast_nullable_to_non_nullable
as int,
  ));
}


}

/// @nodoc


class McpEvent_Connected extends McpEvent {
  const McpEvent_Connected({required this.client}): super._();
  

 final  String client;

/// Create a copy of McpEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$McpEvent_ConnectedCopyWith<McpEvent_Connected> get copyWith => _$McpEvent_ConnectedCopyWithImpl<McpEvent_Connected>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is McpEvent_Connected&&(identical(other.client, client) || other.client == client));
}


@override
int get hashCode {
    return Object.hash(runtimeType,client);
}

@override
String toString() {
    return 'McpEvent.connected(client: $client)';
}


}

/// @nodoc
abstract mixin class $McpEvent_ConnectedCopyWith<$Res> implements $McpEventCopyWith<$Res> {
  factory $McpEvent_ConnectedCopyWith(McpEvent_Connected value, $Res Function(McpEvent_Connected) _then) = _$McpEvent_ConnectedCopyWithImpl;
@useResult
$Res call({
 String client
});




}
/// @nodoc
class _$McpEvent_ConnectedCopyWithImpl<$Res>
    implements $McpEvent_ConnectedCopyWith<$Res> {
  _$McpEvent_ConnectedCopyWithImpl(this._self, this._then);

  final McpEvent_Connected _self;
  final $Res Function(McpEvent_Connected) _then;

/// Create a copy of McpEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? client = null,}) {
  return _then(McpEvent_Connected(
client: null == client ? _self.client : client // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class McpEvent_Disconnected extends McpEvent {
  const McpEvent_Disconnected({required this.client}): super._();
  

 final  String client;

/// Create a copy of McpEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$McpEvent_DisconnectedCopyWith<McpEvent_Disconnected> get copyWith => _$McpEvent_DisconnectedCopyWithImpl<McpEvent_Disconnected>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is McpEvent_Disconnected&&(identical(other.client, client) || other.client == client));
}


@override
int get hashCode {
    return Object.hash(runtimeType,client);
}

@override
String toString() {
    return 'McpEvent.disconnected(client: $client)';
}


}

/// @nodoc
abstract mixin class $McpEvent_DisconnectedCopyWith<$Res> implements $McpEventCopyWith<$Res> {
  factory $McpEvent_DisconnectedCopyWith(McpEvent_Disconnected value, $Res Function(McpEvent_Disconnected) _then) = _$McpEvent_DisconnectedCopyWithImpl;
@useResult
$Res call({
 String client
});




}
/// @nodoc
class _$McpEvent_DisconnectedCopyWithImpl<$Res>
    implements $McpEvent_DisconnectedCopyWith<$Res> {
  _$McpEvent_DisconnectedCopyWithImpl(this._self, this._then);

  final McpEvent_Disconnected _self;
  final $Res Function(McpEvent_Disconnected) _then;

/// Create a copy of McpEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? client = null,}) {
  return _then(McpEvent_Disconnected(
client: null == client ? _self.client : client // ignore: cast_nullable_to_non_nullable
as String,
  ));
}


}

/// @nodoc


class McpEvent_Stopped extends McpEvent {
  const McpEvent_Stopped({this.message}): super._();
  

 final  String? message;

/// Create a copy of McpEvent
/// with the given fields replaced by the non-null parameter values.
@JsonKey(includeFromJson: false, includeToJson: false)
@pragma('vm:prefer-inline')
$McpEvent_StoppedCopyWith<McpEvent_Stopped> get copyWith => _$McpEvent_StoppedCopyWithImpl<McpEvent_Stopped>(this, _$identity);



@override
bool operator ==(Object other) {
    return identical(this, other) || (other.runtimeType == runtimeType&&other is McpEvent_Stopped&&(identical(other.message, message) || other.message == message));
}


@override
int get hashCode {
    return Object.hash(runtimeType,message);
}

@override
String toString() {
    return 'McpEvent.stopped(message: $message)';
}


}

/// @nodoc
abstract mixin class $McpEvent_StoppedCopyWith<$Res> implements $McpEventCopyWith<$Res> {
  factory $McpEvent_StoppedCopyWith(McpEvent_Stopped value, $Res Function(McpEvent_Stopped) _then) = _$McpEvent_StoppedCopyWithImpl;
@useResult
$Res call({
 String? message
});




}
/// @nodoc
class _$McpEvent_StoppedCopyWithImpl<$Res>
    implements $McpEvent_StoppedCopyWith<$Res> {
  _$McpEvent_StoppedCopyWithImpl(this._self, this._then);

  final McpEvent_Stopped _self;
  final $Res Function(McpEvent_Stopped) _then;

/// Create a copy of McpEvent
/// with the given fields replaced by the non-null parameter values.
@pragma('vm:prefer-inline') $Res call({Object? message = freezed,}) {
  return _then(McpEvent_Stopped(
message: freezed == message ? _self.message : message // ignore: cast_nullable_to_non_nullable
as String?,
  ));
}


}

// dart format on
