//! Havana-Web's controllers (`org.alexdev.http.controllers`), one module per Java package/class
//! group. [`handle`] maps a route's Java handler name to its port.

mod account;
mod api;
mod client;
mod club;
mod groups;
mod habblets;
mod habblets2;
mod hk_catalogue;
mod hk_content;
mod hk_core;
mod hk_tools;
mod hk_users;
mod homes;
mod me;
mod minimail;
mod pages;
mod profile;
mod recovery;
mod register;
mod shop;
mod site;
mod tags;

use super::routes_table::HOUSEKEEPING_PATH;
use super::tpl::Tpl;
use super::{Conn, WebResult, beans};

/// Java: `BaseController.handleRoute`, run for every request before its route.
pub fn base(conn: &mut Conn) -> WebResult {
    let route = conn.route.clone();
    if conn.request_handled
        && conn.world().settings.read().bool("maintenance")
        && !route.starts_with("/api")
        && !route.starts_with("/maintenance")
        && !route.starts_with(&format!("/{HOUSEKEEPING_PATH}"))
    {
        conn.redirect("/maintenance");
        return Ok(());
    }
    if !route.starts_with("/api")
        && let Some(proto) = conn.header("x-forwarded-proto")
        && let Some(host) = conn.header("host")
        && proto.eq_ignore_ascii_case("http")
    {
        let mut target = format!("https://{host}");
        if !conn.uri.starts_with('/') {
            target.push('/');
        }
        target.push_str(&conn.uri.clone());
        conn.moved_permanently(&target);
        return Ok(());
    }
    if conn.request_handled {
        if conn.is_authenticated() {
            authenticated_route(conn);
        } else {
            check_remember_cookie(conn);
        }
    }
    Ok(())
}

/// `BaseController.handleAuthenticatedRoute`.
fn authenticated_route(conn: &mut Conn) {
    let now = beans::now();
    let route = conn.route.clone();
    let mut s = conn.session();
    if route == "/client" {
        s.set("lastRequest", (now + REAUTHENTICATE_TIME).to_string());
    }
    if s.contains("lastRequest") {
        if now > s.long("lastRequest") {
            s.set("clientAuthenticate", true);
        }
    } else {
        s.set("clientAuthenticate", false);
    }
}

/// Java: `SessionUtil.REAUTHENTICATE_TIME` (30 minutes).
pub const REAUTHENTICATE_TIME: i64 = 30 * 60;
pub const REMEMBER_TOKEN: &str = "remember_token";
pub const REMEMBER_TOKEN_AGE: i64 = 31 * 24 * 3600;
pub const MACHINE_ID_COOKIE: &str = "SECURITY_KEY";

/// Java: `SessionUtil.checkCookie`: the "remember me" token logs the session in.
fn check_remember_cookie(conn: &mut Conn) {
    let Some(token) = conn.cookie(REMEMBER_TOKEN).map(str::to_string) else {
        return;
    };
    if token.trim().is_empty() {
        return;
    }
    let user = conn.world().store.users().ok().and_then(|users| {
        users
            .into_iter()
            .find(|u| u.remember_token.as_deref() == Some(token.as_str()))
    });
    match user {
        Some(u) if u.id > 0 => {
            {
                let mut s = conn.session();
                s.set("authenticated", true);
                s.set("captcha.invalid", false);
                s.set("user.id", u.id as i64);
            }
            if conn.uri == "/home" || conn.uri == "/index" || conn.uri == "/" {
                conn.redirect("/me");
            }
        }
        _ => {
            {
                let mut s = conn.session();
                s.delete("user.id");
                s.delete("authenticated");
            }
            conn.set_cookie(REMEMBER_TOKEN, "", Some(0));
        }
    }
}

/// Java: `XSSUtil.clear`.
pub fn xss_clear(conn: &mut Conn) {
    let mut s = conn.session();
    if s.contains("xssKey") || s.contains("xssSeed") || s.contains("xssRequested") {
        s.delete("xssSeed");
        s.delete("xssKey");
        s.delete("xssRequested");
    }
}

/// Java: `HtmlUtil.removeHtmlTags`.
pub fn remove_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('<') {
        out.push_str(&rest[..i]);
        match rest[i..].find('>') {
            Some(j) => rest = &rest[i + j + 1..],
            None => {
                out.push_str(&rest[i..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Java: `HousekeepingBinder`. TODO(web): housekeeping (phase 5).
pub fn housekeeping_binder(conn: &mut Conn, tpl: &mut Tpl) {
    super::hk::binder(conn, tpl);
}

/// Runs the controller for a route's Java handler; `false` when it is not ported yet.
pub fn handle(conn: &mut Conn, handler: &str) -> Result<bool, super::WebError> {
    match handler {
        "HomepageController::homepage" => site::homepage(conn)?,
        "HomepageController::maintenance" => site::maintenance(conn)?,
        "AccountController::submit" => account::submit(conn)?,
        "AccountController::securityCheck" => account::security_check(conn)?,
        "AccountController::me" => me::me(conn)?,
        "AccountController::welcome" => account::welcome(conn)?,
        "AccountController::reauthenticate" => account::reauthenticate(conn)?,
        "AccountController::banned" => account::banned(conn)?,
        "RegisterController::register" => register::register(conn)?,
        "RegisterController::registerCancelled" => register::cancelled(conn)?,
        "RegisterController::captcha" => register::captcha_image(conn)?,
        "RecoveryController::forgot" => recovery::forgot(conn)?,
        "RecoveryController::recovery" => recovery::recovery(conn)?,
        "RecoveryController::activate" => recovery::activate(conn)?,
        "RoomSelectionController::confirm" => habblets::room_selection_confirm(conn)?,
        "RoomSelectionController::create" => habblets::room_selection_create(conn)?,
        "RoomSelectionController::hide" => habblets::room_selection_hide(conn)?,
        "FeedController::removeFeedItem" => habblets::remove_feed_item(conn)?,
        "FeedController::nextgift" => habblets::next_gift(conn)?,
        "FeedController::giftqueueHide" => habblets::gift_queue_hide(conn)?,
        "NameCheckController::namecheck" => habblets::name_check(conn)?,
        "CommunityController::community" => pages::community(conn)?,
        "NewsController::articles" => pages::articles(conn)?,
        "NewsController::fansites" => pages::fansites(conn)?,
        "NewsController::events" => pages::events(conn)?,
        "GamesController::games" => pages::games(conn)?,
        "GamesController::games_all_time" => pages::games_all_time(conn)?,
        "GamesController::personalhighscores" => pages::personal_highscores(conn)?,
        "FaqController::faq" => pages::faq(conn)?,
        "SiteController::pixels" => pages::pixels(conn)?,
        "ProfileController::profile" => profile::profile(conn)?,
        "ProfileController::passwordupdate" => profile::password_update(conn)?,
        "ProfileController::emailupdate" => profile::email_update(conn)?,
        "ProfileController::characterupdate" => profile::character_update(conn)?,
        "ProfileController::action" => profile::action(conn)?,
        "ProfileController::profileupdate" => profile::profile_update(conn)?,
        "ProfileController::securitysettingupdate" => profile::security_update(conn)?,
        "ProfileController::wardrobeStore" => profile::wardrobe_store(conn)?,
        "ProfileController::verify" => profile::verify(conn)?,
        "ProfileController::send_email" => profile::send_email(conn)?,
        "ProfileController::club" => club::profile_club(conn)?,
        "FriendManagementController::editCategory" => profile::edit_category(conn)?,
        "FriendManagementController::createcategory" => profile::create_category(conn)?,
        "FriendManagementController::deletecategory" => profile::delete_category(conn)?,
        "FriendManagementController::viewCategory" => profile::view_category(conn)?,
        "FriendManagementController::updateCategoryOptions" => {
            profile::update_category_options(conn)?
        }
        "FriendManagementController::movefriends" => profile::move_friends(conn)?,
        "FriendManagementController::deletefriends" => profile::delete_friends(conn)?,
        "ClubController::club" => club::club(conn)?,
        "ClubController::clubTryout" => club::club_tryout(conn)?,
        "ClubController::habboClubGift" => club::club_gift(conn)?,
        "HabboClubHabblet::confirm" => club::confirm(conn)?,
        "HabboClubHabblet::subscribe" => club::subscribe(conn)?,
        "HabboClubHabblet::enddate" => club::end_date(conn)?,
        "HabboClubHabblet::reminderRemove" => club::reminder_remove(conn)?,
        "CreditsController::credits" => club::credits(conn)?,
        "CreditsController::transactions" => club::history(conn)?,
        "MinimailController::loadMessages" => habblets2::load_messages(conn)?,
        "MinimailController::recipients" => habblets2::recipients(conn)?,
        "MinimailController::preview" => habblets2::preview(conn)?,
        "MinimailController::sendMessage" => habblets2::send_message(conn)?,
        "MinimailController::loadMessage" => habblets2::load_message(conn)?,
        "MinimailController::deleteMessage" => habblets2::delete_message(conn)?,
        "MinimailController::undeleteMessage" => habblets2::undelete_message(conn)?,
        "MinimailController::emptyTrash" => habblets2::empty_trash(conn)?,
        "QuickmenuController::groups" => habblets2::quickmenu_groups(conn)?,
        "QuickmenuController::friends" => habblets2::quickmenu_friends(conn)?,
        "QuickmenuController::rooms" => habblets2::quickmenu_rooms(conn)?,
        "ProxyHabblet::moreInfo" => habblets2::more_info(conn)?,
        "ProxyHabblet::minimail" => habblets2::cproxy(conn)?,
        "ProxyHabblet::clearhand" => habblets2::clear_hand(conn)?,
        "ProxyHabblet::token_generate" => habblets2::token_generate(conn)?,
        "UpdateMottoController::updatemotto" => habblets2::update_motto(conn)?,
        "EventController::loadEvents" => habblets2::load_events(conn)?,
        "NavigationComponent::navigation" => habblets2::navigation(conn)?,
        "InviteController::inviteLink" => habblets2::invite_link(conn)?,
        "InviteController::searchContent" => habblets2::search_content(conn)?,
        "InviteController::confirmAddFriend" => habblets2::confirm_add_friend(conn)?,
        "InviteController::addFriend" => habblets2::add_friend(conn)?,
        "InviteController::add" => habblets2::add(conn)?,
        "VerifyController::get" => habblets2::verify_get(conn)?,
        "VerifyController::clear" => habblets2::verify_clear(conn)?,
        "AdvertisementController::getImg" => habblets2::ad_image(conn)?,
        "AdvertisementController::getUrl" => habblets2::ad_url(conn)?,
        "VoucherController::redeemVoucher" => shop::redeem_voucher(conn)?,
        "CollectablesController::collectables" => shop::collectables(conn)?,
        "CollectablesController::confirm" => shop::confirm(conn)?,
        "CollectablesController::purchase" => shop::purchase(conn)?,
        "TagController::tag" => tags::tag(conn)?,
        "TagController::search" => tags::search(conn)?,
        "TagController::tagsearch" => tags::tag_search(conn)?,
        "TagController::add" => tags::add(conn)?,
        "TagController::remove" => tags::remove(conn)?,
        "TagController::mytaglist" => tags::my_tag_list(conn)?,
        "TagController::tagfight" => tags::tag_fight(conn)?,
        "TagController::tagmatch" => tags::tag_match(conn)?,
        "TagController::remove_all_tags" => tags::remove_all(conn)?,
        "ClientController::shockwaveclient" => client::shockwave_client(conn)?,
        "ClientController::flashClient" => client::flash_client(conn)?,
        "ClientController::betaClient" => client::beta_client(conn)?,
        "ClientController::clientInstallShockwave" => client::install_shockwave(conn)?,
        "ClientController::updateHabboCount" => client::update_habbo_count(conn)?,
        "ClientController::blank" => client::blank(conn)?,
        "ClientController::client_error" => client::client_error(conn)?,
        "ClientController::client_connection_failed" => client::connection_failed(conn)?,
        "TicketController::get" => api::ticket(conn)?,
        "TicketController::login" => api::login(conn)?,
        "AccountController::logout" => account::logout(conn)?,
        "AccountController::login_popup" => account::login_popup(conn)?,
        "ClientController::client" => client::client(conn)?,
        "HomesController::home" => homes::home(conn)?,
        "HomesController::inventory" => homes::inventory(conn)?,
        "HomesController::inventoryItems" => homes::inventory_items(conn)?,
        "HomesController::inventoryPreview" => homes::inventory_preview(conn)?,
        "HomesController::startEditingSession" => homes::start_editing_session(conn)?,
        "HomesController::cancelEditingSession" => homes::cancel_editing_session(conn)?,
        "HomesController::save" => homes::save(conn)?,
        "HomesController::tagList" => homes::tag_list(conn)?,
        "StoreController::main" => homes::store_main(conn)?,
        "StoreController::items" => homes::store_items(conn)?,
        "StoreController::preview" => homes::store_preview(conn)?,
        "StoreController::purchaseConfirm" => homes::purchase_confirm(conn)?,
        "StoreController::backgroundWarning" => homes::background_warning(conn)?,
        "StoreController::purchaseStickers" => homes::purchase(conn, crate::web::homes::STICKER)?,
        "StoreController::purchaseBackgrounds" => {
            homes::purchase(conn, crate::web::homes::BACKGROUND)?
        }
        "StoreController::purchaseStickieNotes" => homes::purchase(conn, crate::web::homes::NOTE)?,
        "NoteEditorController::noteEditor" => homes::note_editor(conn)?,
        "NoteEditorController::notePreview" => homes::note_preview(conn)?,
        "NoteEditorController::search" => homes::link_search(conn)?,
        "NoteEditorController::place" => homes::note_place(conn)?,
        "NoteEditorController::stickieEdit" => homes::stickie_edit(conn)?,
        "NoteEditorController::stickieDelete" => homes::stickie_delete(conn)?,
        "WidgetController::editWidget" => homes::widget_edit(conn)?,
        "WidgetController::placeSticker" => homes::place_sticker(conn)?,
        "WidgetController::placeWidget" => homes::place_widget(conn)?,
        "WidgetController::removeSticker" => homes::remove_sticker(conn)?,
        "WidgetController::removeWidget" => homes::remove_widget(conn)?,
        "GuestbookController::preview" => homes::guestbook_preview(conn)?,
        "GuestbookController::add" => homes::guestbook_add(conn)?,
        "GuestbookController::remove" => homes::guestbook_remove(conn)?,
        "GuestbookController::configure" => homes::guestbook_configure(conn)?,
        "RateController::rate" => homes::rate(conn)?,
        "RateController::resetRating" => homes::reset_rating(conn)?,
        "BadgesController::badgepaging" => homes::badge_paging(conn)?,
        "TraxController::selectSong" => homes::select_song(conn)?,
        "TraxController::getSong" => homes::get_song(conn)?,
        "FriendsWidgetController::friendsearchpaging" => homes::friend_search_paging(conn)?,
        "FriendsWidgetController::avatarinfo" => homes::avatar_info(conn)?,
        "MemberWidgetController::membersearchpaging" => homes::member_search_paging(conn)?,
        "GroupController::viewGroup" => groups::view_group(conn)?,
        "GroupController::startEditingSession" => groups::start_editing_session(conn)?,
        "GroupController::cancelEditingSession" => groups::cancel_editing_session(conn)?,
        "GroupController::saveEditingSession" => groups::save_editing_session(conn)?,
        "GroupController::groupinfo" => groups::group_info(conn)?,
        "GroupHabbletController::groupCreateForm" => groups::group_create_form(conn)?,
        "GroupHabbletController::purchaseConfirmation" => groups::purchase_confirmation(conn)?,
        "GroupHabbletController::purchaseAjax" => groups::purchase_ajax(conn)?,
        "GroupHabbletController::groupSettings" => groups::group_settings(conn)?,
        "GroupHabbletController::checkGroupUrl" => groups::check_group_url(conn)?,
        "GroupHabbletController::updateGroupSettings" => groups::update_group_settings(conn)?,
        "GroupHabbletController::showBadgeEditor" => groups::show_badge_editor(conn)?,
        "GroupHabbletController::updateGroupBadge" => groups::update_group_badge(conn)?,
        "GroupHabbletController::confirmDeleteGroup" => groups::confirm_delete_group(conn)?,
        "GroupHabbletController::deleteGroup" => groups::delete_group(conn)?,
        "GroupMemberController::join" => groups::join(conn)?,
        "GroupMemberController::confirmLeave" => groups::confirm_leave(conn)?,
        "GroupMemberController::leave" => groups::leave(conn)?,
        "GroupMemberController::memberlist" => groups::member_list(conn)?,
        "GroupMemberController::confirmRevokeRights" => groups::confirm_revoke_rights(conn)?,
        "GroupMemberController::revokeRights" => groups::revoke_rights(conn)?,
        "GroupMemberController::confirmGiveRights" => groups::confirm_give_rights(conn)?,
        "GroupMemberController::giveRights" => groups::give_rights(conn)?,
        "GroupMemberController::confirmRemove" => groups::confirm_remove(conn)?,
        "GroupMemberController::remove" => groups::remove(conn)?,
        "GroupMemberController::confirmAccept" => groups::confirm_accept(conn)?,
        "GroupMemberController::accept" => groups::accept(conn)?,
        "GroupMemberController::confirmDecline" => groups::confirm_decline(conn)?,
        "GroupMemberController::decline" => groups::decline(conn)?,
        "GroupFavouriteController::confirmselectfavourite" => {
            groups::confirm_select_favourite(conn)?
        }
        "GroupFavouriteController::selectfavourite" => groups::select_favourite(conn)?,
        "GroupFavouriteController::confirmdeselectfavourite" => {
            groups::confirm_deselect_favourite(conn)?
        }
        "GroupFavouriteController::deselectfavourite" => groups::deselect_favourite(conn)?,
        "GroupTagController::addGroupTag" => groups::add_group_tag(conn)?,
        "GroupTagController::listGroupTag" => groups::list_group_tag(conn)?,
        "GroupTagController::removeGroupTag" => groups::remove_group_tag(conn)?,
        "GroupDiscussionsController::viewDiscussions" => groups::view_discussions(conn)?,
        "GroupDiscussionsController::viewDiscussionsPage" => groups::view_discussions_page(conn)?,
        "DiscussionController::viewDiscussion" => groups::view_discussion(conn)?,
        "DiscussionActionsController::newtopic" => groups::new_topic(conn)?,
        "DiscussionActionsController::savetopic" => groups::save_topic(conn)?,
        "DiscussionActionsController::pingsession" => groups::ping_session(conn)?,
        "DiscussionActionsController::opentopicsettings" => groups::open_topic_settings(conn)?,
        "DiscussionActionsController::confirm_delete_topic" => groups::confirm_delete_topic(conn)?,
        "DiscussionActionsController::deletetopic" => groups::delete_topic(conn)?,
        "DiscussionActionsController::savetopicsettings" => groups::save_topic_settings(conn)?,
        "DiscussionActionsController::updatepost" => groups::update_post(conn)?,
        "DiscussionActionsController::deletepost" => groups::delete_post(conn)?,
        "DiscussionActionsController::savepost" => groups::save_post(conn)?,
        "DiscussionPreviewController::previewtopic" => groups::preview_topic(conn)?,
        "DiscussionPreviewController::previewpost" => groups::preview_post(conn)?,
        "HousekeepingController::dashboard" => hk_core::dashboard(conn)?,
        "HousekeepingController::login" => hk_core::login(conn)?,
        "HousekeepingController::logout" => hk_core::logout(conn)?,
        "HousekeepingCommandsController::ban" => hk_core::command_ban(conn)?,
        "HousekeepingBansController::bans" => hk_core::bans(conn)?,
        "HousekeepingBansController::unban" => hk_core::unban(conn)?,
        "HousekeepingTransactionsController::search" => hk_core::transactions(conn)?,
        "HousekeepingTransactionsController::item_lookup" => hk_core::item_lookup(conn)?,
        "HousekeepingConfigController::configurations" => hk_core::configurations(conn)?,
        "HousekeepingUsersController::search" => hk_users::search(conn)?,
        "HousekeepingUsersController::view" => hk_users::view(conn)?,
        "HousekeepingUsersController::save" => hk_users::save(conn)?,
        "HousekeepingUsersController::action" => hk_users::action(conn)?,
        "HousekeepingUsersController::create" => hk_users::create(conn)?,
        "HousekeepingUsersController::edit" => hk_users::edit(conn)?,
        "HousekeepingUsersController::imitate" => hk_users::imitate(conn)?,
        "HousekeepingToolsController::hotelAlert" => hk_tools::hotel_alert(conn)?,
        "HousekeepingToolsController::chatlogs" => hk_tools::chatlogs(conn)?,
        "HousekeepingToolsController::audit" => hk_tools::audit(conn)?,
        "HousekeepingToolsController::roomView" => hk_tools::room_view(conn)?,
        "HousekeepingToolsController::roomAction" => hk_tools::room_action(conn)?,
        "HousekeepingNewsController::articles" => hk_content::articles(conn)?,
        "HousekeepingNewsController::create" => hk_content::create(conn)?,
        "HousekeepingNewsController::delete" => hk_content::delete(conn)?,
        "HousekeepingNewsController::edit" => hk_content::edit(conn)?,
        "HousekeepingNewsController::preview_news_article" => {
            hk_content::preview_news_article(conn)?
        }
        "HousekeepingAdsController::roomads" => hk_content::room_ads(conn)?,
        "HousekeepingAdsController::delete" => hk_content::delete_ad(conn)?,
        "HousekeepingAdsController::create" => hk_content::create_ad(conn)?,
        "HousekeepingRoomBadgesController::badges" => hk_content::room_badges(conn)?,
        "HousekeepingRoomBadgesController::delete" => hk_content::delete_room_badge(conn)?,
        "HousekeepingRoomBadgesController::create" => hk_content::create_room_badge(conn)?,
        "HousekeepingCatalogueFrontpageController::edit" => hk_content::catalogue_frontpage(conn)?,
        "HousekeepingCatalogueController::pages" => hk_catalogue::pages(conn)?,
        "HousekeepingCatalogueController::editPage" => hk_catalogue::edit_page(conn)?,
        "HousekeepingCatalogueController::deletePage" => hk_catalogue::delete_page(conn)?,
        "HousekeepingCatalogueController::items" => hk_catalogue::items(conn)?,
        "HousekeepingCatalogueController::editItem" => hk_catalogue::edit_item(conn)?,
        "HousekeepingCatalogueController::deleteItem" => hk_catalogue::delete_item(conn)?,
        "HousekeepingCatalogueController::packages" => hk_catalogue::packages(conn)?,
        "HousekeepingCatalogueController::editPackage" => hk_catalogue::edit_package(conn)?,
        "HousekeepingCatalogueController::deletePackage" => hk_catalogue::delete_package(conn)?,
        "HousekeepingCatalogueController::saleBadges" => hk_catalogue::sale_badges(conn)?,
        "HousekeepingCatalogueController::deleteSaleBadge" => {
            hk_catalogue::delete_sale_badge(conn)?
        }
        "HousekeepingCatalogueController::collectables" => hk_catalogue::collectables(conn)?,
        "HousekeepingCatalogueController::editCollectable" => hk_catalogue::edit_collectable(conn)?,
        "HousekeepingCatalogueController::deleteCollectable" => {
            hk_catalogue::delete_collectable(conn)?
        }
        "HousekeepingInfobusController::polls" => hk_catalogue::polls(conn)?,
        "HousekeepingInfobusController::create_polls" => hk_catalogue::create_poll(conn)?,
        "HousekeepingInfobusController::delete" => hk_catalogue::delete_poll(conn)?,
        "HousekeepingInfobusController::edit" => hk_catalogue::edit_poll(conn)?,
        "HousekeepingInfobusController::view_results" => hk_catalogue::view_results(conn)?,
        "HousekeepingInfobusController::clear_results" => hk_catalogue::clear_results(conn)?,
        "HousekeepingInfobusController::send_poll" => hk_catalogue::send_poll(conn)?,
        "HousekeepingInfobusController::close_event" => hk_catalogue::close_event(conn)?,
        "HousekeepingInfobusController::door_status" => hk_catalogue::door_status(conn)?,
        _ => return Ok(false),
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags() {
        assert_eq!(remove_html_tags("a<b>c</b>d<e"), "acd<e");
    }
}
